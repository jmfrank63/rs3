#!/usr/bin/env bash
# Repo guard checks for CI (and local use), D-05. Exits non-zero if any guard fails.
# Every guard runs and reports `ok [x]` or `FAIL [x]`, so one run shows every violation.
# The tags [a]..[h] map one-to-one to D-05 and always print in that order.
# Root: $CI_GUARDS_ROOT, defaulting to this script's parent directory.
set -euo pipefail
ROOT="${CI_GUARDS_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$ROOT"

failures=0
fail() { echo "ci-guards: FAIL [$1] $2" >&2; failures=$((failures + 1)); }
pass() { echo "ci-guards: ok   [$1] $2"; }

# (a)-(f) are added before (g).

# (g) exactly one workflow; no error suppression, server env, lint-lowering flags or
# event-context interpolation in it; and its structure matches check-workflow.py.
# PyYAML: in CI ($CI set) a missing PyYAML skips only the structure check, with a
# printed skip line; locally the structure check is mandatory. Nothing is pip-installed.
shopt -s nullglob
wf=(.github/workflows/*.yml .github/workflows/*.yaml)
shopt -u nullglob
if [[ ${#wf[@]} -ne 1 || ${wf[0]:-} != .github/workflows/ci.yml ]]; then
  fail g "expected only .github/workflows/ci.yml, found: ${wf[*]:-none}"
elif hits=$(grep -nE 'continue-on-error|SERVER\.|RUSTFLAGS|cap-lints|\$\{\{ *github\.event\.' "${wf[@]}"); then
  fail g "forbidden workflow content:"$'\n'"$hits"
elif ! python3 -c 'import yaml' 2>/dev/null; then
  if [[ -n "${CI:-}" ]]; then
    echo "ci-guards: skip [g-structure] PyYAML unavailable"
    pass g "single ci.yml, no forbidden content (structure check skipped: PyYAML unavailable in CI)"
  else
    fail g "PyYAML is required locally for the workflow structure check (python3 -c 'import yaml' failed)"
  fi
elif ! msg=$(python3 scripts/check-workflow.py 2>&1); then
  fail g "workflow structure:"$'\n'"$msg"
else pass g "single ci.yml, structure ok"; fi

# (h) is added after (g).

if (( failures )); then echo "ci-guards: $failures guard(s) failed" >&2; exit 1; fi
echo "ci-guards: all guards passed"
