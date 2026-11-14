# Milestone: The coding loop as a durable goal

Read `AGENTS.md`, `docs/distributed-design.md` and `prototypes/iso-agents/README.md` first. Steps 1 to 6 of the
build order are done: a goal can run a real model (Qwen3-0.6B via candle), commit every token before anyone sees
it, and continue on another node after a crash. This milestone ports the prototype's coding loop into rs3 as a new
kind of goal. The model writes TypeScript, tsc-rs (built to WebAssembly) type-checks it, a sandbox runs its tests,
and the errors go back to the model until the code passes. Every step of every iteration is journaled, so a loop cut
off on one node finishes on another exactly as it would have without the crash.

## Goal

`POST /goals` accepts a coding goal: a task, a spec and an iteration limit. A node runs the loop from
`prototypes/iso-agents/src/agent/loop.ts` in Rust, using the rs3 model seam for the model, the vendored
`ts_rust.wasm` for type-checking and a fresh V8 isolate for running the tests. `GET /goals/{id}` shows each
iteration's reply, diagnostics and test results, and the outcome. T3 Code sees the loop over ACP: the model's text
as message chunks, and each check and run as a tool call. Killing the owner mid-loop loses nothing and repeats
nothing that was committed. Plain text goals and every existing test are unchanged.

## Facts to build on

- The loop to port is `prototypes/iso-agents/src/agent/loop.ts` and `extractor.ts`:
  - **Prompt:** the system prompt (`SYSTEM_PROMPT`); the user message holds the task and spec; then a window of the
    last 3 feedback messages.
  - **Reply:** a JSON contract `{"explanation", "files"}` with exactly one script-style file. One corrective retry
    is allowed for a malformed reply.
  - **Gates:** a type error skips the sandbox. Diagnostics are fed back as `file:line:col TSxxxx: message`. Failed
    tests and logs are fed back as in `formatSandboxFeedback`.
  - **Outcomes:** `success`, `max_iterations`, `run_deadline`, `llm_error`, `llm_malformed`.
- **The checker:**
  - **Module:** `prototypes/iso-agents/src/typecheck-wasm/ts_rust.wasm`, a `wasm32-wasip1` build of the
    pingdotgg/ts-rust fork, branch `multi-run` at `c1b8218339d0` (base pin `569df52`). It is 5,195,794 bytes, with
    sha256 `e05783c80872375e5f5a578d8f535da44abfa6a9de4c1ac904ebefc43b242a71`. `docs/ts-rust-wasm.md` there still
    lists an older build's hash.
  - **Interface:** it imports `ts_host.fs` and `ts_host.fs_take`, plus a small WASI subset; every other import is an
    ENOSYS stub. It exports `memory`, `ts_input`, `ts_run`, `ts_output` and `ts_output_len`.
  - **Reference host:** `src/typecheck-wasm/shim.ts` and `memfs.ts` are the reference host, including the rule that
    every write stamps a fresh monotonic mtime (the fork's parse cache keys on it). `guard.ts` and `mapping.ts` turn
    the output into diagnostics.
  - **Library files:** the guest compiler options and the es2022 library files come from `src/typecheck/` (from
    typescript 6.0.3).
- **Checker memory:** the fork leaks in a long-lived instance. Measured on oc-bastion in KB per run over 600 runs,
  with fixes that are not yet in the vendored build:

  | Pattern | KB/run without fixes | KB/run with fixes |
  |---|---|---|
  | One file edited per run | 4.5 | 0.84 |
  | Type errors | 4.9 | 1.0 |
  | New filename each run | 7.2 | 5.2 |

  - The native build leaks 0.31 to 0.42 MB per run, so the checker runs only as wasm, never linked natively.
  - The 32-bit symbol index space only grows, by 512 to 1536 per run. It lasts about 2.8 million runs, and a
    128 MB instance fills after about 23,000 runs, so recycling after 50 runs is far inside both limits. Details and
    the fork commit are in `M-coding-loop-ts-rust-handoff.md`.
  - The prototype recycles its instance after 50 runs and after any trap.
- **Sandbox:** the prototype runs guests in QuickJS-wasm with an injected `test`, `expect` and `assert` harness
  (`src/sandbox/harness.ts`). rs3 already runs scripts in fresh deno_core V8 runtimes, with a timeout and no store
  access when no ops are registered (`docs/js-ops.md`).
- **Seams to reuse:**
  - `Record::InferenceStart`, the token batches, the restore path, `Command::Decline` on an engine mismatch, ACP
    session updates and `tests/common/model.rs` all exist.
  - `Goals::generated_tokens()` counts every sampled token, including any sampled again.

## Required behaviour

- **A coding goal.**
  - **Request:** `POST /goals` with `{"kind": "code", "task", "spec", "max_iterations", "seed", "budget"}`, where
    `budget` is the token limit per reply.
  - **Plain goals:** a goal without `kind` stays a text goal, exactly as now.
  - **Response:** `GET /goals/{id}` returns `status`, `outcome` and an `iterations` array. Each iteration holds
    `reply`, `files`, `diagnostics`, `tests`, `logs` and `feedback`.
- **The loop.** Same prompt, contract, gates, feedback format, window and outcomes as the prototype. The model gets
  the chat template of the system prompt, the user message and the feedback window. Each iteration's prompt is fixed
  when it is committed. The run deadline becomes a total token budget (`max_iterations × budget`, plus corrective
  retries), because wall time is not deterministic across nodes.
- **Journal every step, and show it only once committed.**
  - **Committing:** an iteration's reply tokens are committed in batches as now. Its check result and its run result
    are each committed as records before they are visible. The outcome is committed last.
  - **On restore:** a node never recomputes a committed step. It finishes the step that was in progress from its
    inputs. A reply goes through the token restore path. A check or a run is computed from the committed files,
    which must give the same result. It then goes on.
  - **On failover:** an uninterrupted loop and one killed at any point give byte-identical `GET /goals/{id}` output.
- **Deterministic steps.** The check depends only on the files and the checker build. The guest sees a fixed
  `Date.now()` and a seeded `Math.random`, has no ops and no network, and is stopped by a deterministic limit
  rather than wall time, so it gives the same result on every node. If V8 cannot interrupt deterministically, use
  the existing timeout and report that difference in the summary. Do not weaken the byte-identical tests.
- **Hosting the checker.**
  - **Runtime:** host `ts_rust.wasm` from Rust with wasmtime (latest stable 50.x; check for a blocker before adding
    it). Port the host side of `shim.ts` and `memfs.ts`. Compile the module once per node.
  - **Instance lifetime:** keep one instance and recycle it after `CHECKER.RECYCLE_AFTER` runs (constant default 50)
    or after any trap, and cap its memory.
  - **Recycling is invisible:** a check gives the same diagnostics on a fresh instance as on one that has served
    many runs.
  - **Wasm only:** the checker never runs natively.
- **Checker identity.**
  - **Hashing:** the node hashes the checker module and the library files with blake3 at start, and a coding goal
    records that hash.
  - **Mismatch:** a node whose checker hash differs declines the goal, just as on a model mismatch.
  - **Files:** copy the module and the library files into the repo, outside `prototypes/`, with a file that records
    where they came from and their hashes.
- **Transpile.** The sandbox needs JavaScript. Use the checker module's own emit if it has one. Otherwise use the
  smallest pure-Rust option, with types stripped and no new C++ dependency. Syntax errors are reported as
  diagnostics.
- **ACP.**
  - **Starting a loop:** a session turn whose prompt starts with `/code` starts a coding goal. Its first line is the
    task and the rest is the spec.
  - **Streaming:** the replies stream as `agent_message_chunk`. Each check and each run is an ACP `tool_call` with
    `tool_call_update`, holding its diagnostics or test results, and completed or failed.
  - **End of turn:** the turn ends with `end_turn` on `success` and `max_tokens` on any other outcome.
  - **Replay:** `session/load` replays all of it byte-identically.
- **Test model.**
  - **Replay model:** loop tests need replies with known content. Add a replay model selected by `MODEL.REPLIES`, a
    JSON file holding a list of reply strings, one per model call. It is a deterministic stand-in like `FakeModel`,
    not a mock of rs3 code. It commits its replies as tokens through the normal path.
  - **Real model:** one acceptance test uses the real Qwen3-0.6B.

## Acceptance tests

All are behavioural. They start real `rs3` processes and bound every wait with a deadline.

1. **Green on the second try.** With replayed replies (first a type error, then a correct file), the goal ends in
   `success` after 2 iterations. Iteration 1 holds the TS diagnostic and no test results. Iteration 2's prompt
   contains that diagnostic as feedback.
2. **Failing tests are fed back.** A reply that type-checks but fails a test sends the failed test's name and error
   back, and the loop ends in `max_iterations` at the limit.
3. **Malformed reply.** One malformed reply followed by a valid one uses the corrective retry. Two malformed replies
   end in `llm_malformed`.
4. **Guest limits.** An endless loop, a throw and a `Date.now()` or `Math.random()` guest all give the same results
   on two fresh nodes. An endless loop is stopped and reported, never hanging the node.
5. **Checker recycling.** On one node, the same files checked as the first run and as run 60 (after recycling, with
   changing files in between) give identical diagnostics. A trap recycles the instance and the next check is
   correct.
6. **Restore mid-loop.** `kill -9` a single node during iteration 2's reply, and again in a second run between a
   committed check and its run. After restart the goal ends byte-identical to an uninterrupted run, and no reply
   token is sampled twice.
7. **Failover.** On 5 nodes, kill the owner and one other node mid-loop. Another node finishes it byte-identically,
   and once sealed the goal reads back with two nodes down.
8. **Checker mismatch.** In a cluster where one node has a different checker hash, that node commits nothing to the
   goal, and the goal finishes on the others.
9. **ACP.** A `/code` turn over ACP shows the replies as message chunks and each check and run as a tool call.
   `session/load` replays it byte-identically.
10. **Real model.** One node with Qwen3-0.6B runs a coding goal for a trivial spec, such as
    `add(a: number, b: number): number`. Its outcome is the same on two fresh nodes. Pick a seed and limit for which
    it reaches `success`, and record them.
11. **Nothing else changes.** Text goals and every existing test pass unchanged.

`scripts/ci-smoke.sh` also runs one replayed coding goal on a 3-node release cluster, kills the leader mid-loop, and
compares the result with a single-node run. Document coding goals, `/code`, `MODEL.REPLIES` and `CHECKER.*` in
`Readme.md`, and add a step 7 to `docs/distributed-design.md`.

## Out of scope

- A new checker build. Use the vendored one; the leak fixes come later as a module swap with a new hash.
- Remote or hosted models, and multi-file guests.
- Tools beyond check and run.
- Changes to `ci.yml`.
- Deleting `prototypes/iso-agents`; decide that after the milestone.
- GPU, growing membership, libvirt or Docker nodes.

Prefer the shortest design that satisfies these tests, and split it into small slices, each with a behaviour you
can demonstrate. A suggested order:

1. The replay model and a single-node loop with the checker hosted and the sandbox, with no durability beyond
   tokens.
2. Committed check and run records, and restore.
3. Checker identity, mismatch and failover.
4. ACP.
5. The real-model test, the smoke test and the docs.
