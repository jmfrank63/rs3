# Agent instructions for rs3

rs3 is a Rust key/value store being turned into a distributed system that runs durable agents in
isolates. Read `docs/distributed-design.md` for the target design and the decisions behind it.

## How to work

- **DNRY, and short solutions.** Reuse what exists. Choose the smallest change that does the job. Add no
  abstraction, option or dependency until a behaviour needs it.
- **TDD.** Write a failing test first, watch it fail, then write the code that makes it pass.
- **Behavioural tests only.** A test states what a caller or client observes: HTTP responses, store contents,
  what survives a crash, the output of a restored stream. Do not test private functions, call order, internal
  structure or mocks of our own code. A test must keep passing through a refactor that preserves behaviour.
- A failure the public API cannot produce, such as disk corruption, may be injected, as long as the test
  asserts what the caller then observes.

## Gates

Run `bash scripts/quality.sh quick` before finishing a task. It runs the repo guards, `cargo fmt --check`,
clippy with warnings as errors, the tests, at least 90% line coverage, and CRAP at most 15.

Lower no threshold, and add no `#[allow]`, `#[ignore]` or mutation exclusion, to get a gate passing. Fix the
code or the test. If that seems impossible, report a blocker.

Before finishing a task that changes Rust code, run mutation testing on that task's change:

```sh
git diff HEAD -- src > target/task.diff   # or the task's commit range
cargo mutants --in-place --no-shuffle --in-diff target/task.diff
```

Kill every surviving mutant with a behavioural test, or remove the code it shows to be unnecessary. An agent's
foreground command is capped at 10 minutes, so split a long run with `--file`. The maintainer runs the full gate
(`bash scripts/quality.sh full <base>`) at the end of each slice. `--in-place` edits source files while it runs,
so never run anything else in this checkout at the same time.

## Machine and commands

- Linux x64, Rust 1.99.1 pinned by `rust-toolchain.toml`. Always pass `--locked` once `Cargo.lock` is updated.
- `scripts/ci-smoke.sh` tests the release binary end to end, and needs `cargo build --release --locked` first.
- Long-running work stays in the foreground. A background task dies with the session and leaves the task incomplete.
- Do not push, and do not edit `.github/workflows/ci.yml`: `scripts/ci-guards.sh` pins its contents.
- `.planning/` is the finished v0.2 record. Do not edit it.
