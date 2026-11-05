#!/usr/bin/env bash
# Quality gates. Usage: quality.sh [quick|full] [base-ref]
# quick: guards, format, clippy (warnings are errors), tests, coverage and CRAP.
# full:  quick plus mutation testing of the Rust code changed since base-ref (default main).
# Thresholds: 90% line coverage, CRAP 15. src/main.rs is excluded because only
# scripts/ci-smoke.sh exercises it, in a separate process.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
mode=${1:-full}
base=${2:-main}
lcov=target/lcov.info
diff=target/mutation.diff

bash scripts/ci-guards.sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo llvm-cov --locked --ignore-filename-regex 'src/main\.rs' \
  --fail-under-lines 90 --lcov --output-path "$lcov"
cargo crap --lcov "$lcov" --exclude 'experiments/**' --exclude 'src/main.rs' \
  --threshold 15 --fail-above --summary

[[ $mode == quick ]] && exit 0
git diff "$base" -- 'src/*.rs' 'src/**/*.rs' >"$diff"
[[ -s $diff ]] || { echo "quality: no Rust changes since $base"; exit 0; }
cargo mutants --in-place --no-shuffle --in-diff "$diff"
