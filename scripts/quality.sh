#!/usr/bin/env bash
# Quality gates. Usage: quality.sh [quick|full|mutants] [base-ref] [cargo-mutants args...]
# quick:   guards, format, clippy (warnings are errors), tests, coverage and CRAP.
# full:    quick plus mutation testing of the Rust code changed since base-ref (default main).
# mutants: only the mutation testing, e.g. `quality.sh mutants HEAD --file src/goals.rs`.
# Thresholds: 90% line coverage, CRAP 15.
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH" # callers such as GSD may run without it
cd "$(dirname "${BASH_SOURCE[0]}")/.."
mode=${1:-full}
base=${2:-main}
shift $(($# < 2 ? $# : 2))
lcov=target/lcov.info
diff=target/mutation.diff

# A mutant left behind by an interrupted cargo mutants run must never pass a gate.
if grep -rln 'changed by cargo-mutants' src tests; then echo "quality: mutated source left in place" >&2; exit 1; fi
if [[ $mode != mutants ]]; then
  bash scripts/ci-guards.sh
  cargo fmt --check
  cargo clippy --all-targets --locked -- -D warnings
  cargo llvm-cov --locked --fail-under-lines 90 --lcov --output-path "$lcov"
  cargo crap --lcov "$lcov" --exclude 'experiments/**' --threshold 15 --fail-above --summary
  [[ $mode == quick ]] && exit 0
fi
# New files count as changed: mark them intent-to-add so git diff shows them.
git ls-files -z --others --exclude-standard -- src | xargs -0 -r git add --intent-to-add
git diff "$base" -- 'src/*.rs' 'src/**/*.rs' >"$diff"
[[ -s $diff ]] || { echo "quality: no Rust changes since $base"; exit 0; }
cargo mutants --in-place --no-shuffle --in-diff "$diff" "$@"
