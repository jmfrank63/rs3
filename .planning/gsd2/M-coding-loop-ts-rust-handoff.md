# ts-rust leak fixes: handoff from oc-bastion (2026-11-14)

Results of the leak-fix work on the tsc-rs fork, for M006 and the later checker swap. Received from the oc-bastion
session "Prototype Agents in Cloudflare Isolates", which is settled. The same text is on oc-bastion at
`~/src/ts-rust-leak-drafts/handoff-to-rs3.md`.

**Limits still in force:** nothing is pushed to `multi-run`, and nothing is posted or edited on pingdotgg/ts-rust
PR #85. Both wait for the user's go-ahead. The two drafts below are unposted.

## Results

**Branch:** `wip/leak-fixes` on `jmfrank63/ts-rust`, at `235958e1eb6c9fe3470055dec892a65bbf76dd31`. It sits on `dee3479` (the as-is snapshot), which is on top of `multi-run`.
- Fix A: an unchanged tsconfig text reuses its parse (`multirun::cached_config_parse`, used from `tsconfig_p1.rs`).
- Fix B: synthetic flow nodes are tagged with their program and freed when the wasm checker pool stops (`checker::free_synthetic_flows_of`, called from `program.rs`).
- Soak harness behind the `ts_wasm` feature `soak` (`crates/ts_wasm/src/soak.rs`, `test_alloc.rs`, `scripts/wasm/soak-runner.mjs`). CI builds without it. Two regression tests: `unchanged_config_is_parsed_once` (native) and `synthetic_flows_are_freed_across_runs` (wasm).
- `core.rs` is unchanged. Editing it invalidates the committed lib snapshot blobs.

**Clone on oc-bastion:** `~/src/ts-rust-leak` (`/home/t3code/src/ts-rust-leak`). Also there: the unmodified base worktree `~/src/ts-rust-base` (`6281c42`) and the TypeScript Go pin `~/src/go-pin`. The branch is pushed, so a fresh clone of `wip/leak-fixes` has the same code.

**Live-byte growth after cleanup** (wasm32, Node WASI, slope over the second half of 600 runs):

| pattern | before | after |
|---|---|---|
| identical rerun | 0 | **0 KB/run** |
| one small file edited | 4.5 | **0.84 KB/run** |
| small edit with type errors | 4.9 | **0.98 KB/run** |
| 300-line file re-parsed each run | 8.1 | **2.8 KB/run** |
| new file name each run | 7.2 | **5.2 KB/run** |

**Symbol ids:** the "id-carry" framing was wrong. The Go-parity symbol ids carried between programs are u64 and harmless. What grows is the 32-bit `SymbolId`/`SymbolTable` *arena index*: each re-bound file version starts new 256-slot lineage chunks on even chunks only, and freed chunks are never reused.

| pattern | symbol / table index per run | runs before 32-bit exhaustion |
|---|---|---|
| identical rerun | 0 / 0 | never |
| small edit, errors, new name | 512 / 512 | about 8.4 M |
| 300-line file re-parsed | 1536 / 512 | about 2.8 M |

Overflow is a clean `symbol overflow` / `table overflow` panic, not corruption. Memory runs out long before that: at the slopes above, a 128 MB isolate fills after about 23 k (new name each run) to 146 k (small edit) runs. M006's recycle default of 50 runs is far inside both limits. The remaining bytes are mostly link-store page tables sized to that index high-water mark.

**Native build:** the brief's "about 280 KB per run" should read **0.31–0.42 MB/run** (300-run soaks: 311 / 393 / 418 / 399 KB/run for 300-line / small edit / new name / errors; 0 for identical reruns). Cause: on native, the re-parse of a changed file puts its nodes in the never-freed `LeakArena`. wasm is not affected. This supports the brief's wasm-only choice.

**Gates** (toolchain 1.95.0, linux-arm64): these all pass:
- fmt; host clippy; the three wasm32 CI clippy forms; `ts_wasm` with `--features soak` on both targets
- `ts_wasm` tests: 12/12 native and 2/2 wasm under the stock `test-runner.mjs`
- the `goport_util` WASI probe; `build.sh`; `npm test`

Workspace tests and `go_baselines` fail exactly the same tests as unmodified `6281c42` on the same machine (8 workspace, 3 `astnav_api`). The cause was not investigated. Two `textedit` tests failed once, then passed on re-runs; they are flaky. Upstream's protected test set (maintainers' testboxes) was not run.

**For M006's later checker swap:** a `ts_rust.wasm` built from `wip/leak-fixes` with `WASM_PROFILE=release WASM_OPT=none scripts/wasm/build.sh` is the candidate module; it gets a new sha256 as the brief already plans.

## Draft 1 (unposted): updated PR #85 body

The current live body can be fetched with `gh pr view 85 -R pingdotgg/ts-rust --json body`.

````markdown
## ts_wasm: one instance serves many tsc runs (multi-program reuse + incremental semantic-diagnostics reuse)

### Summary

`ts_rust.wasm` now serves **many type-check runs from one instance**: runs load program
versions through the existing multi-program machinery (`register_program_version` /
`enter_program` / `release_program`), unchanged files keep their published `GoFile`s and
their semantic diagnostics, and identical requests replay from a fingerprint cache.

**Scope:** the diff spans `crates/ts_goport/src/program/multirun.rs` (new), two small
leak fixes in `checker/extras.rs` + `program.rs` and `frontend/tsoptions/tsconfig_p1.rs`,
and `crates/ts_wasm` (lib/host/tests, plus a `soak` measurement feature and
`scripts/wasm/soak-runner.mjs`). There is **no core.js or core.rs change**. The ABI is
frozen (`ts_input`/`ts_run`/`ts_output`).

### Host contract: none

The parse cache is keyed on **content hash** (the language server's `parsecache.rs` scheme;
bundled libs skip the hash) — hosts need no mtime stamping. Per-request **cwd and
case-sensitivity changes are rejected explicitly** (exit 1) instead of silently using the
first run's mode. The complexity guard question from review is handled host-side by
consumers (see Known limits).

### Memory (re-measured: live bytes, two leaks fixed)

The earlier figures here were the module's linear-memory size
(`memory.buffer.byteLength`). That counts allocator slack and never shrinks, so it can
not tell retention from fragmentation. The soaks now count **live allocator bytes**
(`crates/ts_wasm/src/soak.rs`, feature `soak`), and the wasm runner attributes each
surviving byte to its allocation site. That found two real leaks, both fixed here:

- **Config re-parse:** every run parsed `tsconfig.json` again, and a config parse is
  static (published for good). An unchanged config text now reuses its parse
  (`multirun::cached_config_parse`).
- **Synthetic flow nodes:** the checker's synthetic flow nodes were freed only when a
  checker worker thread ended. wasm runs checkers on the loading thread, which never ends.
  They are now tagged with their program and freed when its pool stops on wasm
  (`checker::free_synthetic_flows_of`). Ids are never reused, so a stale read panics.

One instance, slope of live bytes over the second half of the soak (wasm32, Node WASI;
before: 300–400-run soaks of the same harness, after: 600 runs):

| pattern | before | after |
|---|---|---|
| identical rerun (replayed) | 0 | **0** |
| one small file edited | 4.5 KB/run | **0.84 KB/run** |
| small edit with type errors | 4.9 KB/run | **0.98 KB/run** |
| 300-line file re-parsed each run | 8.1 KB/run | **2.8 KB/run** |
| new file name each run | 7.2 KB/run | **5.2 KB/run** |

Most of what is left is not retained data but the link-store page tables, sized by the
32-bit symbol/table index high-water mark, which only grows (at least 512 indices per
re-bound file per run; see the follow-up comment). The new-name pattern also keeps one
config parse per distinct config text. Each soak test fails above a cap of about 1.5–2× these
slopes. Hosts that hold an instance for a long time should still recycle on a run budget
until the index space is bounded (at these slopes a 128 MB isolate fills after about
23 k–146 k runs). Hashing note: cache lookup and fingerprinting hash file contents per
run. The measured cost is included in the warm numbers, but hosts serving many large
files should size for it.

The native build is not affected by these numbers but leaks much more in the same soaks
(0.31–0.42 MB/run): there, the re-parse of a changed file allocates its nodes in the
never-freed `LeakArena`. That is outside this PR's wasm scope and noted for follow-up.

### Performance (in-isolate, workerd 1.20261006.1, linux-arm64)

| scenario | warm changed-guest | warm identical |
|---|---|---|
| fresh instance per run (main) | 147–181 ms | 147–181 ms |
| this PR, held instance (v1) | 70–93 ms | ~90 ms |
| this PR + incremental diag reuse (HEAD) | **5–13 ms** | **~1 ms** |

Flat vs guest size through 1000 lines (the checker now re-checks only what a change can
reach). Crossover vs the pure-JS checker in the same isolate: JS is faster at ≤~100 lines,
this branch is faster from ~300 lines up (~1.9× at 1000 lines). Verifier:
`scripts/verify-warm.mjs` / `scripts/verify-scaling.mjs` in
https://github.com/jmfrank63/iso-agents (the consuming agent runtime).

### Known limits

- Very deep expressions still overflow the wasm stack (~depth 600–1200; long binary
  operator chains included) — tracked upstream as #99; main is affected the same way.
- npm `runTsc` still constructs a new instance per call; wiring multi-run into the npm
  entry is follow-up work.
- The 32-bit symbol/table arena index space only grows (freed lineage chunks are never
  reused). Overflow is an explicit `symbol overflow` panic after about 8.4 M small-file
  edits (2.8 M re-binds of a 300-line file) per instance; memory runs out first.

### Validation

- Review blockers: all three addressed (content-hash keying, eviction + freeable parses,
  case-flag rejection with regression tests).
- On the pinned 1.95.0 (linux-arm64): fmt; clippy `-D warnings` on the host and in the
  three wasm32 CI forms, and `ts_wasm` with `--features soak` on both targets;
  `cargo test -p ts_wasm` 12/12 native and 2/2 wasm under `scripts/wasm/test-runner.mjs`,
  including the new `unchanged_config_is_parsed_once` and
  `synthetic_flows_are_freed_across_runs`; the WASI probe; the wasm package build and
  `npm test`.
- Workspace tests and `go_baselines` (skipping `test_local`): the same failing tests
  as the base revision `6281c42` built and run on the same host (8 workspace, 3
  `astnav_api`), so none come from this change. Their cause on that host is not
  investigated. The goport protected set runs on the maintainers' testboxes and has not
  been run for this revision.
- 600-run live-byte soaks per pattern (table above).
- From the earlier revision (`6281c42`), not re-run for the leak fixes: diagnostics
  byte-parity with cache off vs on (80 replies across fixtures), the diff.mjs subset vs
  native, the 46-step-equivalent adversarial probe, and consumer integration (5 live
  harnesses, ~250 assertions, both checker modes).
````

## Draft 2 (unposted): comment on PR #85 about the index space (the "id-carry" comment)

````markdown
Follow-up on the memory review, with measurements: what is left after two leak fixes is not a leak in the usual sense, but the 32-bit symbol/table arena index space only ever growing. I'd like your view on the direction before I change it, since watch mode and the language server share this code.

**Measured** (one wasm instance under Node WASI, 600 runs per pattern, live bytes from a counting allocator, so retention and not fragmentation):

| pattern | live bytes, second half | lineage symbol index | lineage table index |
|---|---|---|---|
| identical rerun (replayed) | 0 B/run | +0/run | +0/run |
| one small file edited | 0.84 KB/run | +512/run | +512/run |
| small edit with type errors | 0.98 KB/run | +512/run | +512/run |
| new file name each run | 5.2 KB/run | +512/run | +512/run |
| 300-line file re-parsed each run | 2.8 KB/run | +1536/run | +512/run |

**Mechanism.** Every bound file version starts a new lineage chunk (`end_chunk`, 256 slots), the lineage uses only the even chunks (`CowChunks::new_interleaved`), and `free_range` turns a dead version's chunks into holes that are never handed out again. So each re-bound file costs at least 512 symbol and 512 table indices per run, whatever its size. The checkers are all freed, but every new checker's link stores keyed by `SymbolId`/`SymbolTable` (`DeclaredTypeLinks`, `ValueSymbolLinks`, ...) build flat page tables up to the current high-water mark. That is the remaining 0.8–2.8 KB/run: it scales with the index space, not with any live data.

**Limit.** `SymbolId` and `SymbolTable` are `u32` and overflow is an explicit panic (`"symbol overflow"` / `"table overflow"`), so nothing corrupts. A single instance reaches it after about 8.4 M small-file edits, or about 2.8 M re-binds of a 300-line file. In practice memory runs out first: at the slopes above, a 128 MB isolate fills after roughly 23 k (new name each run) to 146 k (small edit) runs.

**Options I see:**
1. Reuse freed lineage chunk indices (a free list in `CowChunks`). This bounds both the index space and the page tables. But a stale index would then alias a live symbol instead of panicking, so every holder of an index into a freed range has to be gone first. The `FREED_LINEAGE`/`LineageSeen` machinery may already give us that point.
2. Make the link-store page tables independent of the high-water mark (a base offset, or one more sparse level). This bounds memory per checker, but the index space still runs out eventually.
3. Keep both as they are and document a run budget for hosts (what the PR says now).

I lean towards 1, gated on the freed-range epoch so a reused chunk is only handed out once no checker or program copy can see the old one. Is that consistent with how you want lineage lifetimes to work in the LS?

The measurement harness is on `jmfrank63/ts-rust` `wip/leak-fixes` (`crates/ts_wasm/src/soak.rs`, feature `soak`, plus `scripts/wasm/soak-runner.mjs`, which attributes each live byte to its allocation site).
````
