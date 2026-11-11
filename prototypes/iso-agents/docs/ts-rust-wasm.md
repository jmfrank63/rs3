# The vendored tsc-rs WASM checker (`src/typecheck-wasm/ts_rust.wasm`)

`crates/ts_wasm` in [pingdotgg/ts-rust](https://github.com/pingdotgg/ts-rust) builds ts_goport —
the Rust port of the TypeScript compiler (tsc 7.1.0-dev lineage) — to `wasm32-wasip1`. The
resulting module runs INSIDE this workerd isolate behind the `Typechecker` seam: select it with
`TSCHECKER=tsc-rs-wasm` (`wrangler dev --var TSCHECKER:tsc-rs-wasm`). Nothing is published on
npm, so the module is vendored here, built from source — since Phase 7, from this repo's own
`multi-run-incr` FORK: one wasm instance serves many `ts_run` calls through upstream's existing
program-version flow (register/enter/release), so unchanged files' parses are reused across
checks.

## Provenance (Phase 7 fork build)

| Item | Value |
|---|---|
| Upstream | pingdotgg/ts-rust (MIT — vendored artifact + adapted shim only; see upstream for the license text) |
| Base pin | `569df525d3adfe55d31f1b027569a51953707480` ("docs(state): followups61 result", depth-1 clone 2026-10-13) |
| Fork | local branch **`multi-run-incr`** @ commit **`5ada636`** in the spike clone `/tmp/tsrust-spike/ts-rust` — local commit only, not pushed anywhere |
| Fork diff | 5 files, **+327/-11** vs the pin: NEW `crates/ts_goport/src/program/multirun.rs` (203 lines — `enable()` marks the process a watch process so re-parses become freeable file versions, `install_program_version()` builds the frontend program + makes the program version from CURRENT, `end_run()` releases the version of two runs ago — exactly two versions are ever alive, `MultiRunHost` = the watch-host stat→mtime→cache parse server); `program.rs` (+2, module decl); `execute/execute_tsc.rs` (+11, `install_program` diverts to `multirun::install_program_version`, `perform_compilation` wraps the run's host); `ts_wasm/src/lib.rs` (+~100/-7, `multirun::enable()` before + `multirun::end_run()` after each `command_line`, `install_os_override` guarded by `os_override_installed()`, native 4-sequential-runs test); `ts_wasm/src/host.rs` (+11/-2, generation-stamped mtime test hook). **ABI FROZEN**: `ts_input`/`ts_run`/`ts_output`/`ts_output_len` unchanged; no `ts_reset` needed |
| Artifact | `src/typecheck-wasm/ts_rust.wasm` — 5,166,826 bytes raw / 2,195,384 gzip (pin artifact was 5,125,434: +0.5%) |
| sha256 | `5a2d5914f95babd740e9686ae397d3883d98718abe43ee8fb4ee9750c1ba6e2f` |
| Built with | rustc/cargo 1.99.0 (stable-aarch64-unknown-linux-gnu, rustup minimal profile), target wasm32-wasip1, binaryen wasm-opt 132 (npm `binaryen@132`, local prefix) |
| Build | `CI=1 bash scripts/wasm/build.sh` in the fork clone (2026-10-13, aarch64 Linux; wasm-opt MUST be on PATH — the first fork build attempt failed exactly there) |
| npm status | **Unpublished; built from source at fork multi-run/5ada636 (base pin 569df52)** — nothing from upstream `npm/wasm` is on the registry |

Fork validation (07-RESEARCH, all four gates): upstream `npm/wasm` suite 14/14 (`node --test
npm/wasm/test/tsc.test.mjs`); `cargo test -p ts_wasm --lib` 2/2 incl. the new multi-run-incr test
(4 sequential runs in one process, changed file re-parsed via a generation-stamped mtime);
held-instance repeat-run probe with EXACT iso-agents guest texts — correct diagnostics on every
run (clean → 2×TS2322 → slugify → fib → clean, +3 alternating), warm **71–123 ms vs 494–539 ms
cold** in workerd; memory **flat at exactly 62,521,344 bytes from run 2 through run 50** (every
run changed guest.ts, so every run re-parsed it); recycle-on-error proven (20,000-deep nested
object type → RangeError → instance dropped → fresh instance serves correctly); their
`scripts/wasm/diff.mjs` vs native tsgo: 16-case subset identical to the pin's results
case-for-case (recorded subset — `query-*`/`hono-*` skipped, inputs absent; no fork regressions).

## Rebuild from source (~6.5 min)

The module is committed, so `npm install` has NO rustup/toolchain dependency. To reproduce or
upgrade the build:

1. `rustup` stable + minimal profile; `rustup target add wasm32-wasip1` (~49 s combined on the
   spike machine).
2. Local binaryen: `npm i binaryen` and put that prefix's `node_modules/.bin` on PATH — wasm-opt
   ≥ 132 AND REACHABLE (`export PATH="$PWD/node_modules/.bin:$PATH"`); the first fork build
   failed exactly on a missing wasm-opt. (A global `npm i -g` may be blocked by prefix
   permissions; the local install is the documented workaround.)
3. Check out the fork: reuse the spike clone — `git -C /tmp/tsrust-spike/ts-rust checkout
   multi-run-incr` (commit `5ada636`). If the clone is gone: `git clone --depth 1
   https://github.com/pingdotgg/ts-rust`, check out base pin `569df52`, re-apply the 5-file fork
   diff (inventory in Provenance above; +327/-11) as a new local `multi-run-incr` commit, and RECORD
   the new commit hash — update this file with it.
4. `CI=1 bash scripts/wasm/build.sh` from the clone root. `CI=1` bypasses upstream's
   `systemd-run` capped runner (no systemd user session needed). Expect ~3 min cargo fat-LTO
   cdylib link (~1.2 GB peak RSS on the final rustc) + ~3 min wasm-opt (`--flatten --rereloop
   -Oz -Oz` then `-s 2 --reorder-functions-by-name --strip-debug --strip-producers`, ~405 MB
   peak). For long-running shells: `setsid` + disown the build, not a bare `nohup &`.
5. Verify: `node --test npm/wasm/test/tsc.test.mjs` (14/14) and `cargo test -p ts_wasm --lib`
   (2/2, includes the multi-run-incr test) inside the clone. If your sandbox injects Node execArgv
   flags, the worker_threads-based tests fail with `ERR_WORKER_INVALID_EXEC_ARGV`; reset
   `process.execArgv = []` first — harness issue, not the module.
6. Copy `npm/wasm/ts_rust.wasm` → `src/typecheck-wasm/ts_rust.wasm`, update the fork commit,
   base pin, sha256, and byte size in this file, then re-run `npm run test:live` (the
   tsc-rs-wasm harness re-proves behavior; `src/typecheck-wasm/shim.ts` asserts the
   import/export surface on first use and fails loudly on a mismatched artifact).

Toolchain-version sensitivity: upstream's docs quote ~4.2 MB for the module (Rust 1.98.1); Rust
1.99.0 produced 5,166,826 bytes here (5,125,434 at the base pin). Record whichever rustc built
YOUR artifact.

## How the worker hosts it (WASM-01/02/03 + FORK-02)

- **Import**: `import wasmModule from "./ts_rust.wasm"` — wrangler's default CompiledWasm rule
  (NO `rules` entry needed; same mechanism as `src/sandbox/quickjs.wasm`, which coexists —
  `npm run bundle:check` ships both). The import is a precompiled `WebAssembly.Module`; the
  only legal instantiation in workerd is with host-supplied imports, which the shim provides.
- **Shim**: `src/typecheck-wasm/shim.ts` is a workerd-safe TypeScript port of upstream
  `npm/wasm/core.js`: `ts_host.fs`/`fs_take` over `memfs.ts`'s in-memory map, a working WASI
  subset (`args_sizes_get`/`args_get`/`environ_sizes_get`/`environ_get`/`clock_time_get`/
  `clock_res_get`/`random_get`/`fd_write`/`fd_fdstat_get`/`fd_prestat_get`/`proc_exit`/
  `sched_yield`/`poll_oneoff`), and ENOSYS (52) stubs wired to EVERY other import the module
  actually declares (imports-driven loop, like core.js). NO Node APIs: core.js's Node-only bits
  (`worker_threads` 256 MB stacks, real fs) are deliberately not ported — the run is synchronous
  on the calling thread.
- **HELD instance per isolate (FORK-02, supersedes Phase 6's fresh-instance-per-check)**: the
  shim's `runTscHeld` keeps ONE lazy singleton `WebAssembly.Instance` (the `WebAssembly.Module`
  is compiled once at bundle load) and serves every check with a `ts_run` on it. The instance is
  recycled — dropped and re-instantiated from the same compiled module, 2–8 ms — after **50
  runs** (env-overridable via `TSCHECKER_WASM_RECYCLE_AFTER`; the live harness forces 2 to walk
  the boundary) and on **ANY throw** out of `ts_run`: a `WasiExit` there is the panic-hook exit
  (normal exits arrive as ts_run's RETURN value — 0 clean, 2 = diagnostics found, NOT an error),
  and non-WasiExit throws are the trap/RangeError class. Post-panic Rust state is undefined (a
  panic between `install_program_version` and `end_run` leaves the version bookkeeping
  mid-flight), so the instance is dropped no matter what; the next check instantiates fresh.
  Probe-measured recycle: 162 ms end-to-end including the fresh cold check.
- **The held fs is the parse-cache contract**: the held instance's `MemoryFileSystem` lives as
  long as the instance, and `memfs.ts` stamps every `writeFile`/`remove` with a fresh monotonic
  `mtimeNs`. The fork's `MultiRunHost` serves an unchanged file's parse from its stat-cache
  (path + mtime) without re-reading/re-parsing, and re-parses only files whose mtime moved — a
  CONSTANT mtime silently serves stale parses (07-RESEARCH negative control: a constant-mtime fs
  returned exit 0/[] for a TS2322 guest it had already parsed clean). Files a check no longer
  has are REMOVED from the held fs so they cannot join `include: ["*.ts"]`.
- **First-cwd freeze**: `install_os_override` runs once per instance, so the first run's cwd is
  every run's cwd (07-RESEARCH state matrix). iso-agents always checks with `cwd: "/"` +
  `-p /app` (constant), asserted by `runTscHeld`; env is pinned per instance the same way (WASI
  environ installs at instantiation), and `CASE_SENSITIVE` wins from the first request —
  iso-agents never sets the case-insensitive flag.
- **Run protocol**: request = `[cwd, flags, ...args].join("\0")` UTF-8 written via
  `ts_input(len)`; `ts_run()` executes (exit code as its RETURN value: 0 clean, 2 = diagnostics
  found); diagnostics JSON comes back from `ts_output()`/`ts_output_len()`; `proc_exit`
  surfaces as a thrown `WasiExit` — abnormal in the held flow, i.e. the recycle trigger above.
- **Libs are INSIDE the module** (embedded LZMA, decoded in-Rust) — no lib assets are vendored
  or seeded by the host. Guest files + a strict/es2022 tsconfig are synced under `/app` on the
  held fs; diagnostics fileNames are mapped back to the repo's rooted form by `mapping.ts`.
- **Flags used**: bit 0 (diagnostics JSON) only. `--watch`/`--lsp`/`--api` do not exist in the
  wasm build.
- **Cost model (07-RESEARCH, workerd)**: cold first run ≈ 0.5–1.2 s; warm held runs
  **71–123 ms** (~4–7×); held steady-state memory **62,521,344 bytes flat (runs 2–50)** vs
  ~57 MB one-shot — the second (previous) program version costs ~5.5 MB, comfortably inside the
  128 MB isolate alongside the JS heap. `runTsc` (fresh instance per call) remains in the shim
  as the Phase 6 fresh-instance path — the bench's "wasm (fresh)" leg reproduces exactly those
  semantics via a `TSCHECKER_WASM_RECYCLE_AFTER:1` boot.

## Known limits (documented, not solved — 06-CONTEXT/07-CONTEXT)

- **Stack depth**: the module links a 32 MiB in-linear-memory shadow stack
  (`-zstack-size=33554432`). With no JSPI in workerd, deep checker recursion can trap the
  instance; with the held instance the trap ALSO recycles it (recycle-on-throw), and the route
  returns the structured 500 envelope. Guests in this prototype are small.
- **Sync CPU-bound run**: spike-measured per request in workerd — cold ≈ 0.5–1.2 s, warm held
  ≈ 70–125 ms, instantiate 2–8 ms. Local `wrangler dev` does not enforce CPU limits;
  production would need a paid plan's 30 s ceiling. The prototype is local-only.
- **Fork is local**: `multi-run-incr` @ `5ada636` exists only in the spike clone — see Rebuild
  step 3 for the recovery path if `/tmp/tsrust-spike` is gone.
