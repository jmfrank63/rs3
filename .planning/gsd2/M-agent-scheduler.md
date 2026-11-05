# Milestone: durable agent scheduler on one node

Read `AGENTS.md` and `docs/distributed-design.md` first. This is step 2 of the build order. `src/journal.rs`
already stores and verifies resumable inference streams, and `src/store/mod.rs` is the durable store.

## Goal

One `rs3` process runs one agent working on several goals in parallel. If the process is killed at any moment and
restarted on the same data, the agent continues without generating a committed token again, and finishes with the
same output as an undisturbed run.

## Required behaviour

- **Goals:** a client creates a goal over HTTP with a priority and a token budget, can read its status and committed
  output, and can cancel it. Goals and their state survive a restart.
- **Fake model:** a deterministic model behind a small inference trait, with a seed and a configurable token rate.
  The same prompt, seed and generator state give the same tokens. No real inference in this milestone.
- **Resumable generation:** tokens are committed to the journal in batches. Clients see only committed tokens. After
  a restart, generation continues from the journal's stored state.
- **Scheduling:** a fixed number of workers runs goals at once. A higher-priority goal takes a worker from a
  lower-priority one, which pauses and later resumes from its committed state. A goal stops at its budget.
- **Script tools in isolates:** a goal can run a stored script through the existing JS executor. Every op result is
  logged, and after a crash the script is replayed against the logged results, so no store write happens twice.
- **Leases:** a worker claims a task with a lease that carries a fencing token, and a stale worker's writes are
  refused. Keep this small. It is the seam for the Raft milestone, which comes later.

## Acceptance tests

All are behavioural and run against the real binary or the public API:

1. `kill -9` at random points during generation, repeated, then restart: output is byte-identical to an undisturbed
   run with the same seed, and no goal is lost.
2. Two goals with different priorities and one worker: the higher one finishes first, and both outputs are complete
   and correct.
3. `kill -9` during a script that writes to the store: after replay each write has happened exactly once.
4. A corrupted journal record is reported for that goal. It never yields wrong output, and other goals continue.
5. A stale lease holder cannot commit anything.

## Out of scope

Raft and multiple nodes, Reed-Solomon, real models, external actions, the T3 Code adapter, replacing the JavaScript
engine, and authentication.

Prefer the shortest design that satisfies these tests. Split it into small slices, each with a demonstrable behaviour.
