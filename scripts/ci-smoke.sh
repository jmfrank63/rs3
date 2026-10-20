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
# A server that dies during startup fails fast with its exit status. Status 139 (SIGSEGV)
# at startup is the x86_64 PKU symptom of V8 platform init; the D-07 fallback handles it
# (move unsafe_use_unprotected_platform to [dependencies] and relax guard (d) together).
set -euo pipefail
ROOT="${CI_SMOKE_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$ROOT"
BIN="${RS3_BIN:-target/release/rs3}"
PORT="${RS3_SMOKE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')}"
BASE="http://127.0.0.1:${PORT}"
LOG="$(mktemp)"
PID=""

cleanup() {
  if [[ -n "$PID" ]]; then kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; fi
  rm -f "$LOG"
}
trap cleanup EXIT

fail() { echo "ci-smoke: FAIL: $*" >&2; echo "--- server log ---" >&2; cat "$LOG" >&2 || true; exit 1; }

[[ -x "$BIN" ]] || fail "$BIN missing; run cargo build --release --locked first"
if (exec 3<>"/dev/tcp/127.0.0.1/${PORT}") 2>/dev/null; then fail "port ${PORT} already in use"; fi

# Config only through the process environment, inline here and never in the workflow.
# Bash cannot export dotted names, so use env; env -i also keeps the run hermetic.
env -i PATH="$PATH" SERVER.HOST=127.0.0.1 SERVER.PORT="$PORT" "$BIN" >"$LOG" 2>&1 &
PID=$!

# Readiness: poll until GET / answers; fail fast (with the exit status) if the server dies,
# e.g. a V8 platform segfault on x86_64 (PKU) shows up as status 139 here.
ready=0
body=""
for _ in $(seq 1 60); do
  if ! kill -0 "$PID" 2>/dev/null; then
    status=0; wait "$PID" || status=$?
    PID=""
    fail "server exited during startup (status ${status})"
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

echo "ci-smoke: OK (${BIN}, port ${PORT})"
