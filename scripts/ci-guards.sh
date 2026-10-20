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

# grep_guard TAG OK-MSG FAIL-MSG GREP-ARGS...: grep exit 0 (hits) fails the guard, 1 (no hits)
# passes it, and anything else (a grep error such as a missing directory) fails it too.
grep_guard() {
  local tag=$1 ok=$2 bad=$3 hits rc
  shift 3
  hits=$(grep "$@" 2>&1) && rc=0 || rc=$?
  case $rc in
    0) fail "$tag" "$bad"$'\n'"$hits" ;;
    1) pass "$tag" "$ok" ;;
    *) fail "$tag" "grep failed (exit $rc):"$'\n'"$hits" ;;
  esac
}

# Rust source roots to scan (src/ always; future dirs if they appear).
rs_roots=(src)
for d in tests benches examples build.rs; do [[ -e $d ]] && rs_roots+=("$d"); done

# (a) no lint-suppression attributes: #[allow(, #![allow(, #[expect(, cfg_attr(.. allow/expect(..)).
grep_guard a "no #[allow]/#![allow]/#[expect]/cfg_attr(..allow..)" "lint-suppression attribute found:" \
  -rnE '#!?\[[[:space:]]*(allow|expect)[[:space:]]*\(|cfg_attr[[:space:]]*\(.*\b(allow|expect)[[:space:]]*\(' \
  "${rs_roots[@]}"

# (b) no ignored tests. A source grep, never `cargo test` output: deno_core's extension!
# macro generates a doctest that cargo test reports as `1 ignored`.
grep_guard b "no #[ignore] / ignored doc blocks" "#[ignore] or ignored doc block found:" \
  -rnE '#\[[[:space:]]*ignore|```[[:alnum:]_,]*ignore' "${rs_roots[@]}"

# (c) Cargo.toml lint levels: unwrap_used/expect_used at deny (or forbid), nothing at allow
# (string or `{ level = ... }` table form), parsed with tomllib rather than grepped.
if msg=$(python3 - <<'PY' 2>&1
import sys, tomllib
with open("Cargo.toml", "rb") as f:
    lints = tomllib.load(f).get("lints", {})
def level(v):
    return v.get("level") if isinstance(v, dict) else v
errs = []
for tool, table in lints.items():
    if not isinstance(table, dict):
        errs.append(f"[lints] {tool} is not a table"); continue
    for name, v in table.items():
        if level(v) == "allow":
            errs.append(f"lints.{tool}.{name} = {level(v)!r}")
clippy = lints.get("clippy", {})
for name in ("unwrap_used", "expect_used"):
    if level(clippy.get(name)) not in ("deny", "forbid"):
        errs.append(f"lints.clippy.{name} must be deny (is {level(clippy.get(name))!r})")
print("\n".join(errs))
sys.exit(1 if errs else 0)
PY
); then pass c "Cargo.toml lints: unwrap_used/expect_used deny, no allow"
else fail c "Cargo.toml lints:"$'\n'"$msg"; fi

# (d) the unprotected V8 platform feature stays on the dev edge only, so the release binary
# keeps the protected platform.
# D-07 fallback: if V8 platform init crashes on the x86_64 runner (PKU; the release smoke
# reports status 139), deno_core's unsafe_use_unprotected_platform feature moves from
# [dev-dependencies] to [dependencies] in Cargo.toml, and this guard is relaxed in that same
# change. Never apply it pre-emptively (03-RESEARCH resolved Q1).
if tree=$(cargo tree --locked --target all -e features,normal -i deno_core 2>/dev/null); then
  if grep -q unsafe_use_unprotected_platform <<<"$tree"; then
    fail d "deno_core feature unsafe_use_unprotected_platform reaches the normal (release) edge"
  else pass d "unsafe_use_unprotected_platform not on the normal edge"; fi
else
  fail d "cargo tree failed:"$'\n'"$(cargo tree --locked --target all -e features,normal -i deno_core 2>&1 >/dev/null)"
fi

# (e) exactly one v8 in the graph. Filter on v8 only: cargo tree -d already lists
# same-version duplicates of libc, serde, syn and others.
if dups=$(cargo tree --locked --target all -d --depth 0 --prefix none -e normal,build,dev 2>/dev/null); then
  if v8dups=$(grep -E '^(v8|rusty_v8) v' <<<"$dups"); then
    fail e "duplicate v8 in cargo tree -d:"$'\n'"$v8dups"
  else pass e "single v8"; fi
else
  fail e "cargo tree failed:"$'\n'"$(cargo tree --locked --target all -d --depth 0 --prefix none -e normal,build,dev 2>&1 >/dev/null)"
fi

# (f) no legacy JS op syntax in requests/ and src/: opSync, Deno.core.ops(), and the five
# store ops without the op_ prefix. The extension name rs3_ext stays allowed.
grep_guard f "no opSync / Deno.core.ops() / un-prefixed rs3_* ops" "legacy op syntax found:" \
  -rnE 'opSync|Deno\.core\.ops\(\)|(^|[^_[:alnum:]])rs3_(list|get|insert|delete|patch)([^_[:alnum:]]|$)' \
  requests src

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

# (h) no repo-level cargo config that could lower lints (rustflags, cap-lints).
shopt -s nullglob
cfg=(.cargo/config*)
shopt -u nullglob
if (( ${#cfg[@]} )); then fail h "repo cargo config present: ${cfg[*]}"
else pass h "no .cargo/config*"; fi

if (( failures )); then echo "ci-guards: $failures guard(s) failed" >&2; exit 1; fi
echo "ci-guards: all guards passed"
