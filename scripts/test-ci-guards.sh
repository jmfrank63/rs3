#!/usr/bin/env bash
# Planted-violation self-test for scripts/ci-guards.sh (D-05). Local only, not a CI step.
#
# Copies the tree into a throwaway scratch directory under target/ (never /tmp), plants one
# violation per case, runs the copied guard script there and requires a non-zero exit plus
# `FAIL [<tag>]`. An unmodified copy must pass first (the control); a dirty control would
# make every planted case meaningless, so it aborts the run.
# Env: CI_GUARDS_SCRATCH (default target/tmp/guard-selftest under the repo).
set -uo pipefail  # deliberately no -e: cases are expected to fail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
SCRATCH="${CI_GUARDS_SCRATCH:-$ROOT/target/tmp/guard-selftest}"
REAL_CARGO="$(command -v cargo)"
REAL_PY="$(command -v python3)"
export REAL_CARGO REAL_PY

cases=0
missed=0
OUT=""
RC=0

# fresh: empty SCRATCH and copy the files the guards read into it.
fresh() {
  rm -rf "$SCRATCH"
  mkdir -p "$SCRATCH"
  cp -r Cargo.toml Cargo.lock rust-toolchain.toml clippy.toml src requests .github scripts "$SCRATCH"/
}

# run_guards ENV-ARGS...: run the copied guards inside SCRATCH; sets OUT and RC.
# Callers pass the CI handling: `-u CI` for normal cases, `CI=true` for the skip case.
run_guards() {
  OUT=$(cd "$SCRATCH" && env "$@" PATH="$SCRATCH/shim:$PATH" bash scripts/ci-guards.sh 2>&1)
  RC=$?
}

# expect_fail TAG LABEL SNIPPET [EXTRA]: plant SNIPPET in a fresh copy and require the guards
# to exit non-zero with `FAIL [TAG]` (and the fixed string EXTRA, when given).
expect_fail() {
  local tag=$1 label=$2 snippet=$3 extra=${4:-}
  cases=$((cases + 1))
  fresh
  if ! (cd "$SCRATCH" && bash -c "$snippet"); then
    echo "MISSED $tag: $label (setup failed)"
    missed=$((missed + 1))
    return
  fi
  run_guards -u CI
  if [[ $RC -ne 0 && "$OUT" == *"FAIL [$tag]"* && ( -z "$extra" || "$OUT" == *"$extra"* ) ]]; then
    echo "caught $tag: $label"
  else
    echo "MISSED $tag: $label"
    printf '%s\n' "$OUT" | sed 's/^/    /'
    missed=$((missed + 1))
  fi
}

PY_SHIM=$(cat <<'SNIP'
mkdir -p shim
cat > shim/python3 <<'EOF'
#!/usr/bin/env bash
# no site-packages: PyYAML unavailable, tomllib and socket still importable
exec "$REAL_PY" -S "$@"
EOF
chmod +x shim/python3
SNIP
)

# --- control: an unmodified copy passes ---------------------------------------------------
fresh
run_guards -u CI
if [[ $RC -ne 0 || "$OUT" != *"ci-guards: all guards passed"* ]]; then
  echo "CONTROL FAILED (unmodified copy does not pass the guards):"
  printf '%s\n' "$OUT" | sed 's/^/    /'
  rm -rf "$SCRATCH"
  exit 1
fi
echo "ok control: unmodified copy passes"

# --- (a) lint-suppression attributes ------------------------------------------------------
expect_fail a '#[allow(dead_code)] in src/lib.rs' \
  'echo "#[allow(dead_code)]" >> src/lib.rs'
expect_fail a '#![allow(clippy::all)] in src/main.rs' \
  'echo "#![allow(clippy::all)]" >> src/main.rs'
expect_fail a '#[cfg_attr(test, allow(unused))]' \
  'echo "#[cfg_attr(test, allow(unused))]" >> src/lib.rs'
expect_fail a '#[expect(unused)]' \
  'echo "#[expect(unused)]" >> src/lib.rs'
expect_fail a '#[allow(unused)] in a new tests/t.rs' \
  'mkdir -p tests && echo "#[allow(unused)] fn t() {}" > tests/t.rs'
expect_fail a 'multi-line #[cfg_attr(..., allow(...))] (rustfmt layout)' \
  'printf "#[cfg_attr(\n    all(test, feature = \"a_rather_long_feature_name\"),\n    allow(clippy::unwrap_used, dead_code)\n)]\nfn planted() {}\n" >> src/lib.rs'
expect_fail a 'whitespace-split # [allow(dead_code)]' \
  'echo "# [allow(dead_code)] fn planted() {}" >> src/lib.rs'
expect_fail a 'whitespace-split # ! [allow(unused)]' \
  'echo "# ! [allow(unused)]" >> src/main.rs'
expect_fail a '#[allow(dead_code)] in a symlinked module file' \
  'mkdir -p outside && echo "#[allow(dead_code)] fn z() {}" > outside/z.rs && ln -s ../outside/z.rs src/z.rs'
expect_fail a '#[allow(dead_code)] in a symlinked module directory' \
  'mkdir -p outside && echo "#[allow(dead_code)] fn z() {}" > outside/z.rs && ln -s ../outside src/linked'

# --- (b) ignored tests ----------------------------------------------------------------------
expect_fail b '#[ignore] on a test' \
  'printf "#[test]\n#[ignore]\nfn planted() {}\n" >> src/lib.rs'
expect_fail b 'doc block fenced as ignore' \
  'printf "/// \x60\x60\x60ignore\n/// planted();\n/// \x60\x60\x60\nfn planted_doc() {}\n" >> src/lib.rs'
expect_fail b '#[cfg_attr(not(feature = "never"), ignore)] on a test' \
  'printf "#[test]\n#[cfg_attr(not(feature = \"never\"), ignore)]\nfn planted() {}\n" >> src/lib.rs'
expect_fail b 'multi-line cfg_attr(..., ignore) on a test' \
  'printf "#[test]\n#[cfg_attr(\n    not(feature = \"never\"),\n    ignore\n)]\nfn planted() {}\n" >> src/lib.rs'
expect_fail b 'doc block fenced as ~~~ignore' \
  'printf "/// ~~~ignore\n/// planted();\n/// ~~~\nfn planted_doc() {}\n" >> src/lib.rs'
expect_fail b 'doc block fenced as ```rust ignore (space-separated)' \
  'printf "/// \x60\x60\x60rust ignore\n/// planted();\n/// \x60\x60\x60\nfn planted_doc() {}\n" >> src/lib.rs'
expect_fail b 'doc block fenced as ```rust,ignore' \
  'printf "/// \x60\x60\x60rust,ignore\n/// planted();\n/// \x60\x60\x60\nfn planted_doc() {}\n" >> src/lib.rs'
expect_fail b '#[cfg(any())] on a test module' \
  'printf "#[cfg(any())]\nmod planted_tests {}\n" >> src/lib.rs'
expect_fail b '[lib] test = false in Cargo.toml' \
  'printf "\n[lib]\ntest = false\n" >> Cargo.toml' 'test = false'
expect_fail b '[lib] doctest = false in Cargo.toml' \
  'printf "\n[lib]\ndoctest = false\n" >> Cargo.toml' 'doctest = false'
expect_fail b '[[test]] harness = false in Cargo.toml' \
  'printf "\n[[test]]\nname = \"planted\"\npath = \"tests/planted.rs\"\nharness = false\n" >> Cargo.toml' 'harness = false'
expect_fail b 'autotests = false in [package]' \
  'sed -i "s/^edition = \"2024\"\$/edition = \"2024\"\nautotests = false/" Cargo.toml && grep -qx "autotests = false" Cargo.toml' 'autotests = false'

# --- (c) lowered lint levels in Cargo.toml --------------------------------------------------
expect_fail c 'unwrap_used = "warn"' \
  'sed -i "s/^unwrap_used = \"deny\"\$/unwrap_used = \"warn\"/" Cargo.toml && grep -qx "unwrap_used = \"warn\"" Cargo.toml'
expect_fail c 'expect_used = { level = "allow", priority = 1 }' \
  'sed -i "s/^expect_used = \"deny\"\$/expect_used = { level = \"allow\", priority = 1 }/" Cargo.toml && grep -q "^expect_used = { level = \"allow\"" Cargo.toml'
expect_fail c 'missing_docs = "allow" under [lints.rust]' \
  'sed -i "s/^unsafe_code = \"forbid\"\$/unsafe_code = \"forbid\"\nmissing_docs = \"allow\"/" Cargo.toml && grep -qx "missing_docs = \"allow\"" Cargo.toml'

# --- (d) unprotected V8 platform on the normal edge -----------------------------------------
expect_fail d 'unsafe_use_unprotected_platform on the [dependencies] deno_core entry' \
  'sed -i "s/^deno_core = \"0.412.0\"\$/deno_core = { version = \"0.412.0\", features = [\"unsafe_use_unprotected_platform\"] }/" Cargo.toml && grep -q "^deno_core = { version = \"0.412.0\", features" Cargo.toml'
expect_fail d 'init_platform(Some(new_unprotected_default_platform(..))) in src/js/mod.rs' \
  'sed -i "s/JsRuntime::init_platform(None);/JsRuntime::init_platform(Some(deno_core::v8::new_unprotected_default_platform(0, false).make_shared()));/" src/js/mod.rs && grep -q new_unprotected_default_platform src/js/mod.rs' \
  'unprotected V8 platform selected in source'
expect_fail d 'init_platform(Some(..)) split over lines in src/js/mod.rs' \
  'sed -i "s/JsRuntime::init_platform(None);/JsRuntime::init_platform(\n        Some(deno_core::v8::new_default_platform(0, false).make_shared()),\n    );/" src/js/mod.rs && grep -q new_default_platform src/js/mod.rs' \
  'unprotected V8 platform selected in source'
expect_fail d 'init_platform(platform) with a variable' \
  'sed -i "s/JsRuntime::init_platform(None);/let platform = None;\n    JsRuntime::init_platform(platform);/" src/js/mod.rs && grep -q "init_platform(platform)" src/js/mod.rs' \
  'unprotected V8 platform selected in source'
expect_fail d 'v8::Platform::new_unprotected in src/lib.rs' \
  'echo "fn planted() { let _ = deno_core::v8::Platform::new_unprotected(0, false); }" >> src/lib.rs' \
  'unprotected V8 platform selected in source'
expect_fail d 'v8::new_custom_platform in src/lib.rs' \
  'echo "fn planted() { let _ = deno_core::v8::new_custom_platform(0, false, true); }" >> src/lib.rs' \
  'unprotected V8 platform selected in source'
expect_fail d 'direct v8::V8::initialize_platform in src/lib.rs' \
  'echo "fn planted(p: deno_core::v8::SharedRef<deno_core::v8::Platform>) { deno_core::v8::V8::initialize_platform(p); }" >> src/lib.rs' \
  'unprotected V8 platform selected in source'

# --- (e) duplicate v8 -----------------------------------------------------------------------
expect_fail e 'cargo shim adds v8 v152.2.0 to cargo tree -d' "$(cat <<'SNIP'
mkdir -p shim
cat > shim/cargo <<'EOF'
#!/usr/bin/env bash
# inject a duplicate v8 into `cargo tree -d` output, pass everything else through
if [[ "${1:-}" == tree && " $* " == *" -d "* ]]; then
  printf 'v8 v150.4.0\n\nv8 v152.2.0\n\n'; exit 0
fi
exec "$REAL_CARGO" "$@"
EOF
chmod +x shim/cargo
SNIP
)"

# --- (f) legacy JS op syntax ----------------------------------------------------------------
expect_fail f 'Deno.core.opSync(...) in a requests/ script' \
  'echo "Deno.core.opSync(\"op_rs3_list\");" >> requests/store.js'
expect_fail f 'Deno.core.ops(); in src/example.js' \
  'echo "Deno.core.ops();" >> src/example.js'
expect_fail f 'Deno.core.ops.rs3_list() (un-prefixed op name)' \
  'echo "Deno.core.ops.rs3_list();" >> requests/sum.js'

# --- (g) workflow hygiene -------------------------------------------------------------------
WF=.github/workflows/ci.yml
expect_fail g 'a second workflow other.yaml' \
  "cp $WF .github/workflows/other.yaml"
expect_fail g 'continue-on-error on the job' \
  "sed -i 's/^    timeout-minutes: 45\$/    timeout-minutes: 45\n    continue-on-error: true/' $WF && grep -q '^    continue-on-error: true' $WF"
expect_fail g 'dotted server-config host key under the workflow env' \
  "sed -i 's/^  CARGO_TERM_COLOR: always\$/  CARGO_TERM_COLOR: always\n  SERVER.HOST: 0.0.0.0/' $WF && grep -q '^  SERVER.HOST' $WF"
expect_fail g 'actions/checkout@v2' \
  "sed -i 's|uses: actions/checkout@v7|uses: actions/checkout@v2|' $WF && grep -q 'checkout@v2' $WF"
expect_fail g 'clippy without --locked' \
  "sed -i 's/run: cargo clippy --all-targets --locked -- -D warnings/run: cargo clippy --all-targets -- -D warnings/' $WF && grep -q 'run: cargo clippy --all-targets -- -D' $WF"
expect_fail g 'runs-on ubuntu-26.04-arm' \
  "sed -i 's/^    runs-on: ubuntu-26.04\$/    runs-on: ubuntu-26.04-arm/' $WF && grep -q 'ubuntu-26.04-arm' $WF"
expect_fail g 'guards run line deleted' \
  "sed -i '/run: bash scripts\/ci-guards.sh/d' $WF && ! grep -q 'run: bash scripts/ci-guards.sh' $WF"
expect_fail g 'cargo test with a single-thread flag appended' \
  "sed -i 's/run: cargo test --locked\$/run: cargo test --locked -- --test-threads=1/' $WF && grep -q 'threads=1' $WF"
expect_fail g 'push.branches filter' \
  "sed -i 's/^  push:\$/  push:\n    branches: [main]/' $WF && grep -q 'branches: \[main\]' $WF"
expect_fail g 'permissions block removed' \
  "sed -i '/^permissions:\$/d; /^  contents: read\$/d' $WF && ! grep -q '^permissions' $WF"
expect_fail g 'if: false on the clippy step' \
  "sed -i 's/^      - name: cargo clippy\$/      - name: cargo clippy\n        if: false/' $WF && grep -q '^        if: false' $WF"
expect_fail g 'shell: sh on the fmt step' \
  "sed -i 's/^      - name: cargo fmt --check\$/      - name: cargo fmt --check\n        shell: sh/' $WF && grep -q '^        shell: sh' $WF"
expect_fail g 'run line echoes the github.event.head_commit.message expression' "$(cat <<'SNIP'
cat >> .github/workflows/ci.yml <<'EOF'

      - name: Echo commit message
        run: echo "${{ github.event.head_commit.message }}"
EOF
SNIP
)"
expect_fail g 'ci.yml deleted (zero workflows)' \
  "rm $WF" 'found: none'
expect_fail g 'PyYAML unavailable outside CI' "$PY_SHIM"

# --- (g, open question 1) PyYAML unavailable in CI: structure check skipped, guards pass ----
cases=$((cases + 1))
fresh
(cd "$SCRATCH" && bash -c "$PY_SHIM")
run_guards CI=true
if [[ $RC -eq 0 && "$OUT" == *"ci-guards: skip [g-structure] PyYAML unavailable"* ]]; then
  echo "ok g-structure-skip: PyYAML unavailable with CI=true skips only the structure check"
else
  echo "MISSED g-structure-skip: PyYAML unavailable with CI=true (rc=$RC)"
  printf '%s\n' "$OUT" | sed 's/^/    /'
  missed=$((missed + 1))
fi

# --- (h) repo cargo config ------------------------------------------------------------------
expect_fail h '.cargo/config.toml sets rustflags with cap-lints' \
  'mkdir -p .cargo && printf "[build]\nrustflags = [\"--cap-lints\", \"allow\"]\n" > .cargo/config.toml'

rm -rf "$SCRATCH"
echo "test-ci-guards: ${cases} cases, ${missed} missed"
if (( missed > 0 )); then exit 1; fi
exit 0
