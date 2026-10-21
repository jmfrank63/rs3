#!/usr/bin/env bash
# Release-binary smoke for CI and local use (D-06). Exercises the production (protected)
# V8 platform path: target/release/rs3 is built without dev-dependency features.
# Env: CI_SMOKE_ROOT (default: this script's parent directory), RS3_BIN (default
# target/release/rs3), RS3_SMOKE_PORT (default: a free 127.0.0.1 port).
#
# Asserts, each with the exact status code and the exact body bytes:
#   GET /                   -> 200 "Welcome to rs3"
#   POST /insert m, boom    -> 200  (m = 6*20, boom = throw new Error("boom"))
#   PATCH /key/m            -> 200 "120"  (stored script run in V8)
#   PATCH /key/boom         -> 422 {"error":"js_error","message":"Uncaught Error: boom"}
#   GET / after JS error    -> 200 "Welcome to rs3", and the server process is still alive
#   POST /insert me, loop   -> 200  (me patches itself, loop = while(true){})
#   PATCH /key/me           -> 422 {"error":"js_depth_limit","message":"PATCH nesting depth limit (4)
#                              exceeded: me -> me -> me -> me -> me"}  (one line in the real body)
#   PATCH /key/loop         -> sent in the background; while it runs:
#     GET /                 -> 200 "Welcome to rs3"
#     PATCH /key/m          -> 200 "120" before the loop answers (only when nproc >= 2, because
#                              the pool has one JS worker per core; skipped with a note otherwise)
#   PATCH /key/loop         -> 504 {"error":"js_timeout","message":"script exceeded the 5000 ms
#                              execution limit"}, at least 4900 ms and under 9000 ms after it was sent
#   PATCH /key/m            -> 200 "120" after the timeout, and the server process is still alive
# The release binary uses the default JS limits: nesting depth 4 and a 5000 ms timeout. Both are
# set in code; there is no environment knob, so the timeout check waits about 5 s. This smoke is
# the only place where the timeout watchdog terminates a script under the protected V8 platform
# on the x86_64 runner (debug test builds use the unprotected platform).
# A server that dies fails the smoke with its exit status, whether it dies during startup or
# during a request. Status 139 (SIGSEGV) is the x86_64 PKU symptom of the protected V8
# platform. V8 isolates are created lazily on the JS workers, so the crash shows up at the
# first script execution (PATCH /key/m), not at startup: look for
# "server died (status 139)". The D-07 fallback handles it (move
# unsafe_use_unprotected_platform to [dependencies] and relax guard (d) together).
set -euo pipefail
ROOT="${CI_SMOKE_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$ROOT"
BIN="${RS3_BIN:-target/release/rs3}"
PORT="${RS3_SMOKE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')}"
BASE="http://127.0.0.1:${PORT}"
LOG="$(mktemp)"
LOOP_OUT="$(mktemp)"
PID=""
LOOP_PID=""

# cleanup: SIGTERM, then SIGKILL after 5 s, so a hung graceful shutdown cannot block the job.
# A still-running background request (LOOP_PID) is killed and reaped first.
cleanup() {
  local i
  if [[ -n "$LOOP_PID" ]]; then
    kill "$LOOP_PID" 2>/dev/null || true
    wait "$LOOP_PID" 2>/dev/null || true
  fi
  if [[ -n "$PID" ]]; then
    kill "$PID" 2>/dev/null || true
    for i in $(seq 1 50); do kill -0 "$PID" 2>/dev/null || break; sleep 0.1; done
    kill -9 "$PID" 2>/dev/null || true
    wait "$PID" 2>/dev/null || true
  fi
  rm -f "$LOG" "$LOOP_OUT"
}
trap cleanup EXIT

# status_msg STATUS: "(status N)", plus the D-07 hint for 139 (SIGSEGV).
status_msg() {
  printf '(status %s)' "$1"
  if (( $1 == 139 )); then printf '; SIGSEGV is the x86_64 V8 PKU symptom: apply the D-07 fallback'; fi
}

# server_exit_note: if the server process has exited, reap it and set NOTE to its exit status,
# else set NOTE to "". Runs in the main shell (wait only works there). A crashing server may
# still be exiting when curl sees the dropped connection, so allow it up to 2 s.
server_exit_note() {
  NOTE=""
  [[ -n "$PID" ]] || return 0
  local st=0 i
  for i in $(seq 1 20); do kill -0 "$PID" 2>/dev/null || break; sleep 0.1; done
  if ! kill -0 "$PID" 2>/dev/null; then
    wait "$PID" || st=$?
    PID=""
    NOTE="; server died $(status_msg "$st")"
  fi
}

# fail MSG: report MSG (plus the server's exit status if it died), dump the log, exit 1.
fail() {
  server_exit_note
  echo "ci-smoke: FAIL: $*${NOTE}" >&2
  echo "--- server log ---" >&2
  cat "$LOG" >&2 || true
  exit 1
}

[[ -x "$BIN" ]] || fail "$BIN missing; run cargo build --release --locked first"
if (exec 3<>"/dev/tcp/127.0.0.1/${PORT}") 2>/dev/null; then fail "port ${PORT} already in use"; fi

# Config only through the process environment, inline here and never in the workflow.
# Bash cannot export dotted names, so use env; env -i also keeps the run hermetic.
env -i PATH="$PATH" SERVER.HOST=127.0.0.1 SERVER.PORT="$PORT" "$BIN" >"$LOG" 2>&1 &
PID=$!

# Readiness: poll until GET / answers; fail fast (with the exit status) if the server dies.
# GET / never touches V8, so a PKU crash normally surfaces at the first PATCH instead (see
# server_exit_note, which every failure runs).
ready=0
body=""
deadline=$((SECONDS + 30))
while (( SECONDS < deadline )); do
  if ! kill -0 "$PID" 2>/dev/null; then
    status=0; wait "$PID" || status=$?
    PID=""
    fail "server exited during startup $(status_msg "$status")"
  fi
  if body=$(curl -s --fail --max-time 2 "$BASE/"); then ready=1; break; fi
  sleep 0.5
done
(( ready )) || fail "server not ready on ${BASE} after 30s"
[[ "$body" == '"Welcome to rs3"' ]] || fail "GET / body: ${body}"
echo "ci-smoke: ok   GET / -> \"Welcome to rs3\""

# req METHOD PATH [BODY]: one request; sets CODE (last output line) and BODY (the rest).
req() {
  local out
  local -a data=()
  if (( $# > 2 )); then data=(--data-binary "$3"); fi
  out=$(curl -sS --max-time 30 -w $'\n%{http_code}' -X "$1" ${data[@]+"${data[@]}"} "$BASE$2") \
    || fail "$1 $2: transport error"
  CODE="${out##*$'\n'}"
  BODY="${out%$'\n'*}"
}

req POST /insert '{"m":"6*20","boom":"throw new Error(\"boom\")"}'
[[ "$CODE" == "200" ]] || fail "POST /insert -> ${CODE} ${BODY}"
echo "ci-smoke: ok   POST /insert -> 200"

req PATCH /key/m
[[ "$CODE" == "200" && "$BODY" == '"120"' ]] || fail "PATCH /key/m -> ${CODE} ${BODY}"
echo "ci-smoke: ok   PATCH /key/m -> 200 \"120\""

req PATCH /key/boom
[[ "$CODE" == "422" && "$BODY" == '{"error":"js_error","message":"Uncaught Error: boom"}' ]] \
  || fail "PATCH /key/boom -> ${CODE} ${BODY}"
echo "ci-smoke: ok   PATCH /key/boom -> 422 js_error"

req GET /
[[ "$CODE" == "200" && "$BODY" == '"Welcome to rs3"' ]] || fail "GET / after JS error -> ${CODE} ${BODY}"
kill -0 "$PID" 2>/dev/null || fail "server died after JS error"
echo "ci-smoke: ok   GET / after JS error -> 200, server alive"

# Depth limit (default 4): a key that patches itself is stopped at the fifth level.
req POST /insert '{"me":"Deno.core.ops.op_rs3_patch(\"me\")","loop":"while(true){}"}'
[[ "$CODE" == "200" ]] || fail "POST /insert me, loop -> ${CODE} ${BODY}"
echo "ci-smoke: ok   POST /insert me, loop -> 200"

req PATCH /key/me
[[ "$CODE" == "422" && "$BODY" == '{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: me -> me -> me -> me -> me"}' ]] \
  || fail "PATCH /key/me -> ${CODE} ${BODY}"
echo "ci-smoke: ok   PATCH /key/me -> 422 js_depth_limit"

# Timeout (default 5000 ms): a never-ending script runs in the background while other
# requests are answered. Its output (body, newline, status code) lands in LOOP_OUT.
curl -sS --max-time 30 -w $'\n%{http_code}' -X PATCH "$BASE/key/loop" >"$LOOP_OUT" &
LOOP_PID=$!
LOOP_START=$(date +%s%3N)
sleep 0.5

req GET /
[[ "$CODE" == "200" && "$BODY" == '"Welcome to rs3"' ]] || fail "GET / during a runaway PATCH -> ${CODE} ${BODY}"
echo "ci-smoke: ok   GET / during a runaway PATCH -> 200"

CPUS=$(nproc)
if (( CPUS >= 2 )); then
  req PATCH /key/m
  [[ "$CODE" == "200" && "$BODY" == '"120"' ]] || fail "PATCH /key/m during a runaway PATCH -> ${CODE} ${BODY}"
  # The loop must still be running: its curl has not exited and has written nothing yet.
  kill -0 "$LOOP_PID" 2>/dev/null && [[ ! -s "$LOOP_OUT" ]] \
    || fail "PATCH /key/loop answered before PATCH /key/m: $(cat "$LOOP_OUT")"
  echo "ci-smoke: ok   PATCH /key/m during a runaway PATCH -> 200 \"120\""
else
  echo "ci-smoke: skip PATCH /key/m during a runaway PATCH (nproc ${CPUS})"
fi

loop_rc=0
wait "$LOOP_PID" || loop_rc=$?
LOOP_MS=$(( $(date +%s%3N) - LOOP_START ))
LOOP_PID=""
(( loop_rc == 0 )) || fail "PATCH /key/loop: transport error (curl status ${loop_rc})"
loop_raw=$(cat "$LOOP_OUT")
LOOP_CODE="${loop_raw##*$'\n'}"
LOOP_BODY="${loop_raw%$'\n'*}"
[[ "$LOOP_CODE" == "504" && "$LOOP_BODY" == '{"error":"js_timeout","message":"script exceeded the 5000 ms execution limit"}' ]] \
  || fail "PATCH /key/loop -> ${LOOP_CODE} ${LOOP_BODY} after ${LOOP_MS} ms"
(( LOOP_MS >= 4900 && LOOP_MS < 9000 )) || fail "PATCH /key/loop -> 504 after ${LOOP_MS} ms (expected 4900 to 8999 ms)"
echo "ci-smoke: ok   PATCH /key/loop -> 504 js_timeout after ${LOOP_MS} ms"

req PATCH /key/m
[[ "$CODE" == "200" && "$BODY" == '"120"' ]] || fail "PATCH /key/m after the timeout -> ${CODE} ${BODY}"
kill -0 "$PID" 2>/dev/null || fail "server died after the timeout"
echo "ci-smoke: ok   PATCH /key/m after the timeout -> 200 \"120\", server alive"

echo "ci-smoke: OK (${BIN}, port ${PORT})"
