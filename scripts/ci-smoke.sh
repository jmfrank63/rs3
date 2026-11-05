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
#                              execution limit"}, at least 4900 ms and under 6500 ms after it was sent,
#                              answered by the watchdog: the server log holds the worker's line
#                              'error: js timeout in "loop" after 5000 ms' and no backstop line
#                              ('JS worker did not reply')
#   PATCH /key/m            -> 200 "120" after the timeout, and the server process is still alive
# Then, on a stored database (generation seeds 11 and 22 with budget 2000, a 40-pass script goal
# `job` that appends to `trail` and bumps `count`; CI_SMOKE_SPIN tunes its JIT-proof spin):
#   reference run           -> both generation outputs, which differ
#   3 x kill -9 and restart -> status 137, goal ids still exist, no committed progress shrinks, each
#                              generation output is a prefix of its reference, trail is 0,1,...,n-1,
#   after the last restart  -> all three goals done; generation output equals the reference byte for
#                              byte (2000 words); script output and count are 40; trail is exact
# The release binary uses the default JS limits: nesting depth 4 and a 5000 ms timeout. Both are
# set in code; there is no environment knob, so the timeout check waits about 5 s. This smoke is
# the only place where the timeout watchdog terminates a script under the protected V8 platform
# on the x86_64 runner (debug test builds use the unprotected platform). The backstop answers the
# same 504 body at about 7000 ms (timeout + 2 s grace) when the watchdog fails to terminate the
# script, so the window ends below it, and the log must show that the worker itself returned
# the timed-out job (it writes its timeout line only then), which proves that worker was freed.
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
DIR="$(mktemp -d)"
PID=""
LOOP_PID=""

# stop_server: SIGTERM, then SIGKILL after 5 s, so a hung graceful shutdown cannot block the job.
stop_server() {
  local i
  [[ -n "$PID" ]] || return 0
  kill "$PID" 2>/dev/null || true
  for i in $(seq 1 50); do kill -0 "$PID" 2>/dev/null || break; sleep 0.1; done
  kill -9 "$PID" 2>/dev/null || true
  wait "$PID" 2>/dev/null || true
  PID=""
}

# cleanup: stop the server. A still-running background request (LOOP_PID) is killed and reaped first.
cleanup() {
  if [[ -n "$LOOP_PID" ]]; then
    kill "$LOOP_PID" 2>/dev/null || true
    wait "$LOOP_PID" 2>/dev/null || true
  fi
  stop_server
  rm -rf "$LOG" "$LOOP_OUT" "$DIR"
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

# start_server [NAME=VALUE...]: start the server in the background (PID), appending to LOG, and wait
# until GET / answers. Config only through the process environment, inline here and never in the
# workflow. Bash cannot export dotted names, so use env; env -i also keeps the run hermetic.
# Readiness: GET / never touches V8, so a PKU crash normally surfaces at the first PATCH instead
# (see server_exit_note, which every failure runs). A server that dies during startup fails.
start_server() {
  local ready=0 deadline status
  env -i PATH="$PATH" SERVER.HOST=127.0.0.1 SERVER.PORT="$PORT" "$@" "$BIN" >>"$LOG" 2>&1 &
  PID=$!
  deadline=$((SECONDS + 30))
  while (( SECONDS < deadline )); do
    if ! kill -0 "$PID" 2>/dev/null; then
      status=0; wait "$PID" || status=$?
      PID=""
      fail "server exited during startup $(status_msg "$status")"
    fi
    if body=$(curl -s --fail --max-time 2 "$BASE/"); then ready=1; break; fi
    sleep 0.1
  done
  (( ready )) || fail "server not ready on ${BASE} after 30s"
  [[ "$body" == '"Welcome to rs3"' ]] || fail "GET / body: ${body}"
  echo "ci-smoke: ok   GET / -> \"Welcome to rs3\""
}

start_server

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
(( LOOP_MS >= 4900 && LOOP_MS < 6500 )) \
  || fail "PATCH /key/loop -> 504 after ${LOOP_MS} ms (expected 4900 to 6499 ms; about 7000 ms means the backstop answered, not the watchdog)"
# The worker writes its timeout line only once the terminated job returned, before it replies.
grep -qF 'error: js timeout in "loop" after 5000 ms' "$LOG" \
  || fail "PATCH /key/loop -> 504, but the worker did not log the timeout: the watchdog did not terminate the script"
if grep -qF 'JS worker did not reply' "$LOG"; then
  fail "the backstop answered PATCH /key/loop; the watchdog did not terminate the script"
fi
echo "ci-smoke: ok   PATCH /key/loop -> 504 js_timeout after ${LOOP_MS} ms, terminated by the watchdog"

req PATCH /key/m
[[ "$CODE" == "200" && "$BODY" == '"120"' ]] || fail "PATCH /key/m after the timeout -> ${CODE} ${BODY}"
kill -0 "$PID" 2>/dev/null || fail "server died after the timeout"
echo "ci-smoke: ok   PATCH /key/m after the timeout -> 200 \"120\", server alive"

# ---- Goals through kill -9 (release binary, protected V8 platform) ----
SPIN="${CI_SMOKE_SPIN:-10000000}"
ITERS=40
BUDGET=2000
SCRIPT="const get=(k)=>Deno.core.ops.op_rs3_get(k);const insert=(o)=>Deno.core.ops.op_rs3_insert(JSON.stringify(o));let x=0;for(let i=0;i<${ITERS};i++){const n=Number(get('count'));const t=get('trail');insert({count:String(n+1),trail:t+i+','});for(let s=0;s<${SPIN};s++)x=(x*31+s)|0;}x===0.5?'x':get('count')"

# jf JSON FIELD: print a top-level field of a JSON document (no jq).
jf() { python3 -I -c 'import json,sys; v=json.loads(sys.argv[1])[sys.argv[2]]; print(v if isinstance(v,str) else json.dumps(v))' "$1" "$2"; }

# goal ID: print the goal JSON, or fail.
goal() {
  req GET "/goals/$1"
  [[ "$CODE" == "200" ]] || fail "GET /goals/$1 -> ${CODE} ${BODY}"
  printf '%s' "$BODY"
}

# new_goal JSON: create a goal and print its id.
new_goal() {
  req POST /goals "$1"
  [[ "$CODE" == "201" ]] || fail "POST /goals $1 -> ${CODE} ${BODY}"
  jf "$BODY" id
}

# wait_done ID: poll to done under a 30 s deadline; fail with the JSON if failed. Prints the goal JSON.
wait_done() {
  local g st dl=$((SECONDS + 30))
  while (( SECONDS < dl )); do
    g=$(goal "$1"); st=$(jf "$g" status)
    [[ "$st" != "failed" ]] || fail "goal $1 failed: ${g}"
    if [[ "$st" == "done" ]]; then printf '%s' "$g"; return 0; fi
    sleep 0.1
  done
  fail "goal $1 not done after 30s: $(goal "$1")"
}

# Phase 1: reference outputs, from a fast uninterrupted server.
stop_server
start_server STORAGE.PATH="$DIR/ref.db" MODEL.TOKENS_PER_SECOND=1000000
REF11=$(jf "$(wait_done "$(new_goal "{\"priority\":0,\"budget\":${BUDGET},\"seed\":11,\"prompt\":\"p\"}")")" output)
REF22=$(jf "$(wait_done "$(new_goal "{\"priority\":0,\"budget\":${BUDGET},\"seed\":22,\"prompt\":\"p\"}")")" output)
[[ -n "$REF11" && "$REF11" != "$REF22" ]] || fail "reference outputs for seeds 11 and 22 must be non-empty and differ"
stop_server
echo "ci-smoke: ok   reference generation outputs differ by seed"

# Phase 2: two generation goals and a script goal run at once and are killed 3 times.
start_server STORAGE.PATH="$DIR/rs3.db" MODEL.TOKENS_PER_SECOND=500 SCHEDULER.WORKERS=3
req POST /insert "$(python3 -I -c 'import json,sys; print(json.dumps({"count":"0","trail":"","job":sys.argv[1]}))' "$SCRIPT")"
[[ "$CODE" == "200" ]] || fail "POST /insert count, trail, job -> ${CODE} ${BODY}"
G1=$(new_goal "{\"priority\":0,\"budget\":${BUDGET},\"seed\":11,\"prompt\":\"p\"}")
G2=$(new_goal "{\"priority\":0,\"budget\":${BUDGET},\"seed\":22,\"prompt\":\"p\"}")
GS=$(new_goal '{"priority":0,"script":"job"}')

# observe: sets O1, O2 (generation outputs) and TRAIL after checking them against the references
# and the exact write sequence, and N (trail entries); the script and generation goals must not be done.
observe() {
  local g1 g2 gs
  g1=$(goal "$G1"); g2=$(goal "$G2"); gs=$(goal "$GS")
  O1=$(jf "$g1" output); O2=$(jf "$g2" output)
  [[ "$REF11" == "$O1"* ]] || fail "goal $G1 output is not a prefix of its reference: ${g1}"
  [[ "$REF22" == "$O2"* ]] || fail "goal $G2 output is not a prefix of its reference: ${g2}"
  req GET /key/trail; [[ "$CODE" == "200" ]] || fail "GET /key/trail -> ${CODE} ${BODY}"

  TRAIL="${BODY#\"}"; TRAIL="${TRAIL%\"}"
  N=$(grep -o ',' <<<"$TRAIL" | wc -l)
  [[ "$TRAIL" == "$(python3 -I -c 'import sys; print("".join(f"{i}," for i in range(int(sys.argv[1]))),end="")' "$N")" ]] \
    || fail "trail is not 0,1,...,n-1, : ${TRAIL}"
  for g in "$g1" "$g2" "$gs"; do
    [[ "$(jf "$g" status)" != "failed" ]] || fail "a goal failed: ${g}"
  done
  OBS_STATUS="$(jf "$g1" status) $(jf "$g2" status) $(jf "$gs" status)"
}

L1=0; L2=0; LN=0
for round in 1 2 3; do
  dl=$((SECONDS + 30))
  while :; do
    observe
    if (( ${#O1} > L1 && ${#O2} > L2 && N > LN )); then break; fi
    (( SECONDS < dl )) || fail "round ${round}: goals did not advance (generation ${#O1}/${#O2} > ${L1}/${L2}, trail ${N} > ${LN})"
    sleep 0.05
  done
  if (( round == 3 )) && [[ "$OBS_STATUS" == *done* ]]; then fail "a goal finished before the last kill (${OBS_STATUS})"; fi
  L1=${#O1}; L2=${#O2}; LN=$N
  kill -0 "$PID" 2>/dev/null || fail "server died before kill ${round}"
  kill -9 "$PID"
  st=0; { wait "$PID"; } 2>/dev/null || st=$?
  PID=""
  (( st == 137 )) || fail "kill ${round}: expected status 137, got $(status_msg "$st")"
  start_server STORAGE.PATH="$DIR/rs3.db" MODEL.TOKENS_PER_SECOND=500 SCHEDULER.WORKERS=3
  for id in "$G1" "$G2" "$GS"; do goal "$id" >/dev/null; done
  observe
  (( ${#O1} >= L1 && ${#O2} >= L2 && N >= LN )) || fail "kill ${round}: committed progress shrank (${#O1}/${#O2}/${N} < ${L1}/${L2}/${LN})"
  echo "ci-smoke: ok   kill -9 ${round}: restarted, goals intact, progress kept (${#O1}/${#O2} chars, ${N} writes)"
done

# Phase 3: everything finishes with the exact outputs and exactly-once writes.
D1=$(wait_done "$G1"); D2=$(wait_done "$G2"); DS=$(wait_done "$GS")
[[ "$(jf "$D1" output)" == "$REF11" ]] || fail "goal $G1 output differs from its reference: ${D1}"
[[ "$(jf "$D2" output)" == "$REF22" ]] || fail "goal $G2 output differs from its reference: ${D2}"
WORDS=$(wc -w <<<"$REF11")
(( WORDS == BUDGET )) || fail "generation output has ${WORDS} words, expected ${BUDGET}"
echo "ci-smoke: ok   generation goals equal their references byte for byte (${BUDGET} words)"
[[ "$(jf "$DS" output)" == "$ITERS" ]] || fail "script goal output: ${DS}"
req GET /key/count; [[ "$CODE" == "200" && "$BODY" == "\"${ITERS}\"" ]] || fail "count -> ${CODE} ${BODY}"
req GET /key/trail
[[ "$BODY" == "\"$(python3 -I -c 'import sys; print("".join(f"{i}," for i in range(int(sys.argv[1]))),end="")' "$ITERS")\"" ]] \
  || fail "trail after the script finished: ${BODY}"
echo "ci-smoke: ok   script goal wrote each of ${ITERS} entries exactly once through 3 kills"

echo "ci-smoke: OK (${BIN}, port ${PORT})"
