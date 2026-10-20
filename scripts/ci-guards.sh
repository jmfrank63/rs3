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

# grep_guard TAG OK-MSG FAIL-MSG CMD...: CMD is `grep ...` or `ml_grep ...`. Exit 0 (hits)
# fails the guard, 1 (no hits) passes it, and anything else (an error such as a missing
# directory) fails it too.
grep_guard() {
  local tag=$1 ok=$2 bad=$3 hits rc
  shift 3
  hits=$("$@" 2>&1) && rc=0 || rc=$?
  case $rc in
    0) fail "$tag" "$bad"$'\n'"$hits" ;;
    1) pass "$tag" "$ok" ;;
    *) fail "$tag" "$1 failed (exit $rc):"$'\n'"$hits" ;;
  esac
}

# ml_grep PYTHON-REGEX PATH...: multi-line search over every file under PATH (grep -R
# semantics: symlinked files and directories are followed, loops are skipped). The regex
# sees whole files, so \s also matches newlines and an attribute split over several lines
# (rustfmt's layout for long attributes) still matches. Prints `file:line: match` per hit,
# with whitespace collapsed; exits 0 on hits, 1 on none, 2 on an error such as a missing
# path or a dangling symlink (grep_guard fails closed on that).
ml_grep() {
  python3 - "$@" <<'PY'
import os, re, sys
pat = re.compile(sys.argv[1])
def walk_error(e):
    raise e
def files(root):
    if not os.path.isdir(root):
        yield root
        return
    seen = set()
    for d, dirs, names in os.walk(root, followlinks=True, onerror=walk_error):
        real = os.path.realpath(d)
        if real in seen:
            dirs[:] = []
            continue
        seen.add(real)
        dirs.sort()
        for n in sorted(names):
            yield os.path.join(d, n)
hits = 0
try:
    for root in sys.argv[2:]:
        for p in files(root):
            with open(p, encoding="utf-8", errors="replace") as fh:
                text = fh.read()
            for m in pat.finditer(text):
                line = text.count("\n", 0, m.start()) + 1
                print(f"{p}:{line}: {' '.join(m.group(0).split())}")
                hits += 1
except OSError as e:
    print(f"ml_grep: {e}", file=sys.stderr)
    sys.exit(2)
sys.exit(0 if hits else 1)
PY
}

# Rust source roots to scan (src/ always; future dirs if they appear).
rs_roots=(src)
for d in tests benches examples build.rs; do [[ -e $d ]] && rs_roots+=("$d"); done

# (a) no lint-suppression attributes: #[allow(, #![allow(, #[expect(, cfg_attr(.. allow/expect(..)),
# with any token spacing (`# ! [allow(`) and across lines (a multi-line cfg_attr is rustfmt's
# own layout, so the fmt gate keeps it). Symlinked files are scanned too.
grep_guard a "no #[allow]/#![allow]/#[expect]/cfg_attr(..allow..)" "lint-suppression attribute found:" \
  ml_grep '#\s*!?\s*\[\s*(?:allow|expect)\s*\(|cfg_attr\s*\([^\]]*?\b(?:allow|expect)\s*\(' \
  "${rs_roots[@]}"

# (b) no ignored or disabled tests. A source scan, never `cargo test` output: deno_core's
# extension! macro generates a doctest that cargo test reports as `1 ignored`.
# Source: #[ignore], cfg_attr(.., ignore), doc fences (``` or ~~~) whose lang string contains
# ignore (rustdoc splits on commas and spaces), and #[cfg(any())] / #[cfg(FALSE)].
# Cargo.toml: test/doctest/harness = false on a target, or autolib/autobins/autotests = false,
# which silently drop whole test targets.
b_errs=""
b_src=$(ml_grep '#\s*\[\s*ignore\b|cfg_attr\s*\([^\]]*?\bignore\b|(?:```|~~~)[^\n`~]*\bignore\b|#\s*\[\s*cfg\s*\(\s*(?:any\s*\(\s*\)|FALSE\b)' \
  "${rs_roots[@]}" 2>&1) && b_rc=0 || b_rc=$?
case $b_rc in
  0) b_errs+=$'\n'"ignored test or doc block found:"$'\n'"$b_src" ;;
  1) ;;
  *) b_errs+=$'\n'"ml_grep failed (exit $b_rc):"$'\n'"$b_src" ;;
esac
if ! b_toml=$(python3 - <<'PY' 2>&1
import sys, tomllib
with open("Cargo.toml", "rb") as f:
    data = tomllib.load(f)
errs = []
for sect in ("lib", "bin", "test", "bench", "example"):
    v = data.get(sect)
    for t in [v] if isinstance(v, dict) else (v if isinstance(v, list) else []):
        if not isinstance(t, dict):
            continue
        for k in ("test", "doctest", "harness"):
            if t.get(k) is False:
                errs.append(f"[{sect}] {t.get('name', '(unnamed)')}: {k} = false")
for k in ("autolib", "autobins", "autotests"):
    if data.get("package", {}).get(k) is False:
        errs.append(f"[package] {k} = false")
print("\n".join(errs))
sys.exit(1 if errs else 0)
PY
); then b_errs+=$'\n'"Cargo.toml disables tests:"$'\n'"$b_toml"; fi
if [[ -z $b_errs ]]; then pass b "no ignored tests / ignored doc blocks / disabled test targets"
else fail b "${b_errs#$'\n'}"; fi

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
# D-07 fallback: if V8 crashes on the x86_64 runner (PKU; the release smoke reports
# "server died (status 139)", usually at the first script execution because isolates are
# created lazily, not at startup), deno_core's unsafe_use_unprotected_platform feature moves from
# [dev-dependencies] to [dependencies] in Cargo.toml, and this guard is relaxed in that same
# change. Never apply it pre-emptively (03-RESEARCH resolved Q1).
# Source code could select the unprotected platform without the feature (the v8 constructors
# are safe fns, so unsafe_code = "forbid" does not stop them), so (d) also scans the Rust
# roots: no unprotected/custom platform constructor, no direct V8 platform initialisation,
# and init_platform only ever gets None (deno_core then picks the protected platform in the
# release build). The D-07 fallback does not need any of these, so the scan stays as is then.
d_errs=""
if tree=$(cargo tree --locked --target all -e features,normal -i deno_core 2>/dev/null); then
  if grep -q unsafe_use_unprotected_platform <<<"$tree"; then
    d_errs+=$'\n'"deno_core feature unsafe_use_unprotected_platform reaches the normal (release) edge"
  fi
else
  d_errs+=$'\n'"cargo tree failed:"$'\n'"$(cargo tree --locked --target all -e features,normal -i deno_core 2>&1 >/dev/null)"
fi
d_src=$(ml_grep '\bnew_unprotected\w*|\bnew_custom_platform\b|\binitialize_platform\b|\binit_platform\s*\(\s*(?!None\s*\)|\))' \
  "${rs_roots[@]}" 2>&1) && d_rc=0 || d_rc=$?
case $d_rc in
  0) d_errs+=$'\n'"unprotected V8 platform selected in source:"$'\n'"$d_src" ;;
  1) ;;
  *) d_errs+=$'\n'"ml_grep failed (exit $d_rc):"$'\n'"$d_src" ;;
esac
if [[ -z $d_errs ]]; then pass d "unsafe_use_unprotected_platform not on the normal edge; protected platform in source"
else fail d "${d_errs#$'\n'}"; fi

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
  grep -RnE 'opSync|Deno\.core\.ops\(\)|(^|[^_[:alnum:]])rs3_(list|get|insert|delete|patch)([^_[:alnum:]]|$)' \
  requests src

# (g) exactly one workflow; no error suppression, server env, lint-lowering flags, gate
# redirection or expression interpolation in it; and its structure matches check-workflow.py.
# The content grep is the only workflow check when PyYAML is missing in CI, so it stands on
# its own: case-insensitive (the config loader accepts SERVER.HOST in any case), and it
# rejects working-directory, job-level or write permissions, checkout ref/repository,
# trigger filters, and every `${{` except the one concurrency group line.
# PyYAML: in CI ($CI set) a missing PyYAML skips only the structure check, with a
# printed skip line; locally the structure check is mandatory. Nothing is pip-installed.
# Every layer runs, so forbidden content does not hide structure errors.
shopt -s nullglob
wf=(.github/workflows/*.yml .github/workflows/*.yaml)
shopt -u nullglob
g_errs=""
g_skip=""
if [[ ${#wf[@]} -ne 1 || ${wf[0]:-} != .github/workflows/ci.yml ]]; then
  g_errs+=$'\n'"expected only .github/workflows/ci.yml, found: ${wf[*]:-none}"
else
  g_pat='continue-on-error|server\.|rustflags|cap-lints|working-directory|write-all'
  g_pat+='|^[[:space:]]+permissions[[:space:]]*:|^[[:space:]]*[a-z-]+[[:space:]]*:[[:space:]]*write[[:space:]]*(#.*)?$'
  g_pat+='|^[[:space:]]+(ref|repository)[[:space:]]*:'
  g_pat+='|^[[:space:]]+(branches|branches-ignore|tags|tags-ignore|paths|paths-ignore|types)[[:space:]]*:'
  hits=$(grep -inE "$g_pat" "${wf[@]}" 2>&1) && rc=0 || rc=$?
  case $rc in
    0) g_errs+=$'\n'"forbidden workflow content:"$'\n'"$hits" ;;
    1) ;;
    *) g_errs+=$'\n'"grep failed (exit $rc):"$'\n'"$hits" ;;
  esac
  hits=$(grep -nF '${{' "${wf[@]}" 2>&1) && rc=0 || rc=$?
  case $rc in
    0) if other=$(grep -vE '^[0-9]+:  group: ci-\$\{\{ github\.ref \}\}$' <<<"$hits"); then
         g_errs+=$'\n'"expression interpolation outside the concurrency group:"$'\n'"$other"
       fi ;;
    1) ;;
    *) g_errs+=$'\n'"grep failed (exit $rc):"$'\n'"$hits" ;;
  esac
  if ! python3 -c 'import yaml' 2>/dev/null; then
    if [[ -n "${CI:-}" ]]; then
      echo "ci-guards: skip [g-structure] PyYAML unavailable"
      g_skip=" (structure check skipped: PyYAML unavailable in CI)"
    else
      g_errs+=$'\n'"PyYAML is required locally for the workflow structure check (python3 -c 'import yaml' failed)"
    fi
  elif ! msg=$(python3 scripts/check-workflow.py 2>&1); then
    g_errs+=$'\n'"workflow structure:"$'\n'"$msg"
  fi
fi
if [[ -n $g_errs ]]; then fail g "${g_errs#$'\n'}"
elif [[ -n $g_skip ]]; then pass g "single ci.yml, no forbidden content$g_skip"
else pass g "single ci.yml, no forbidden content, structure ok"; fi

# (h) no repo-level config that could lower lints (rustflags, cap-lints) or disable the
# `cargo fmt --check` gate. The project uses rustfmt defaults, so any rustfmt.toml or
# .rustfmt.toml (at the root, or in a source dir, where rustfmt also looks) fails, e.g.
# `disable_all_formatting = true` makes cargo fmt --check pass unformatted code. So does a
# #[rustfmt::skip] / #![rustfmt::skip] attribute in the Rust roots.
h_errs=""
shopt -s nullglob
cfg=(.cargo/config*)
shopt -u nullglob
if (( ${#cfg[@]} )); then h_errs+=$'\n'"repo cargo config present: ${cfg[*]}"; fi
fmtcfg=$( { find -L . -maxdepth 1 \( -name rustfmt.toml -o -name .rustfmt.toml \) -print \
  && find -L "${rs_roots[@]}" \( -name rustfmt.toml -o -name .rustfmt.toml \) -print; } 2>&1) \
  && rc=0 || rc=$?
if (( rc )); then h_errs+=$'\n'"find failed (exit $rc):"$'\n'"$fmtcfg"
elif [[ -n $fmtcfg ]]; then h_errs+=$'\n'"rustfmt config present (the project uses rustfmt defaults):"$'\n'"$fmtcfg"; fi
h_src=$(ml_grep '\brustfmt\s*::\s*skip\b' "${rs_roots[@]}" 2>&1) && rc=0 || rc=$?
case $rc in
  0) h_errs+=$'\n'"rustfmt::skip found:"$'\n'"$h_src" ;;
  1) ;;
  *) h_errs+=$'\n'"ml_grep failed (exit $rc):"$'\n'"$h_src" ;;
esac
if [[ -z $h_errs ]]; then pass h "no .cargo/config*, no rustfmt config, no rustfmt::skip"
else fail h "${h_errs#$'\n'}"; fi

if (( failures )); then echo "ci-guards: $failures guard(s) failed" >&2; exit 1; fi
echo "ci-guards: all guards passed"
