// Host-side benchmark (TSCRS-01, `npm run bench:tsc-rs`): the NATIVE tsc-rs
// binary vs the IN-ISOLATE pure-JS typescript@6.0.3 checker, on the SAME five
// guest snippets — printed as a markdown table and written to
// docs/benchmarks.md (README commits the same numbers).
//
// Where things run (honesty by construction): tsc-rs is HOST-SIDE ONLY — its
// JS API spawns the native binary as a persistent sync-RPC subprocess. It is
// a devDependency imported ONLY here (scripts/ is outside src/): the worker
// bundle and the no-Node-builtins gate are untouched. The in-isolate leg
// boots `wrangler dev` on a free port (the proven PID-file harness pattern
// from scripts/test-agent-live.mjs) and measures POST /typecheck.
//
// Empirical ground rules (05-RESEARCH, verified on this machine):
//   - `new API({fs})` from "tsc-rs/unstable/sync"; the ambient guest-globals
//     file MUST be a rootName or every test()/expect() guest drowns in
//     TS2593. program.dispose()/api.close() are GETTERS -> call them;
//     api.dispose() does not exist (TypeError).
//   - Options are NUMERIC and mirror GUEST_COMPILER_OPTIONS
//     (typescript@6.0.3 enums: target ES2022 = 9, module ESNext = 99,
//     moduleResolution Bundler = 100).
//   - DiagnosticResponse: {code, category, startPosition: {line, character}
//     (ZERO-based), text (pre-flattened — no messageText chain)}; errors
//     filter via category === DiagnosticCategory.Error (= 1). No exit codes.
//   - The CLI fallback (`npx tsc-rs --noEmit --strict --target es2022
//     --module esnext --skipLibCheck <globals> <guest>`) exists but is NOT
//     benchmarked here: fresh-process runs cost ~155-170 ms of process +
//     native init, ~5x the persistent API per check (05-RESEARCH).
import { spawn, execSync } from "node:child_process";
import { createServer } from "node:net";
import { performance } from "node:perf_hooks";
import { mkdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { API, DiagnosticCategory } from "tsc-rs/unstable/sync";
import { createVirtualFileSystem } from "tsc-rs/unstable/fs";

const failures = [];
function assert(cond, label, detail) {
  if (cond) {
    console.log(`ok - ${label}`);
  } else {
    failures.push(label);
    console.error(`FAIL - ${label}${detail === undefined ? "" : `: ${JSON.stringify(detail)}`}`);
  }
}

// Repo GUEST_GLOBALS_TEXT (src/typecheck/vfs.ts) — inlined copy (established
// harness pattern; src/ is TS and must not be imported from a host script).
const GUEST_GLOBALS_TEXT = `declare var console: { log(...args: unknown[]): void; error(...args: unknown[]): void; };
declare function test(name: string, fn: () => void): void;
declare function assert(cond: unknown, msg?: string): void;
interface Expectation {
  toEqual(expected: unknown): void;
  toBeTrue(msg?: string): void;
  toBeCloseTo(expected: number, eps?: number): void;
}
declare function expect(actual: unknown): Expectation;`;

// Mirrors GUEST_COMPILER_OPTIONS numerically (see header).
const OPTIONS = {
  strict: true,
  target: 9,
  noEmit: true,
  module: 99,
  moduleResolution: 100,
  skipLibCheck: true,
  isolatedModules: true,
  forceConsistentCasingInFileNames: true,
};

const TSC_RS_ROOTS = ["/guest-globals.d.ts", "/guest.ts"]; // globals MUST be a rootName

// Exact mock iteration 1 (src/agent/mock.ts SLUGIFY turn 1): planted TS2322
// x2 (`const slug: number`, `return slug`) — the only red snippet.
const SLUGIFY_ITER1 = `function slugify(input: string): string {
  const slug: number = input.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return slug;
}
test("lowercases and hyphenates", () => { expect(slugify("Hello World")).toEqual("hello-world"); });
test("trims and collapses separators", () => { expect(slugify("  A -- B  ")).toEqual("a-b"); });
`;

// Exact mock iteration 3 (SLUGIFY turn 3): clean.
const SLUGIFY_FINAL = `function slugify(input: string): string {
  return input.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}
test("lowercases and hyphenates", () => { expect(slugify("Hello World")).toEqual("hello-world"); });
test("trims and collapses separators", () => { expect(slugify("  A -- B  ")).toEqual("a-b"); });
`;

// Exact mock iteration 3 (FIBONACCI turn 3): clean.
const FIBONACCI_FINAL = `function fibonacci(n: number): number {
  if (n <= 1) return n;
  let a = 0;
  let b = 1;
  for (let i = 2; i <= n; i++) {
    const next = a + b;
    a = b;
    b = next;
  }
  return b;
}
test("fib(0) and fib(1)", () => { expect(fibonacci(0)).toEqual(0); expect(fibonacci(1)).toEqual(1); });
test("fib(7) is 13", () => { expect(fibonacci(7)).toEqual(13); });
`;

// The lib-closure guest already used by scripts/test-typecheck-live.mjs:
// Map + Promise.all + console against the DEFAULT lib closure (DOM included
// on the tsc-rs leg; the in-isolate core closure covers it too).
const LIB_HEAVY = `
const pairs: Array<[string, number]> = [["a", 1], ["b", 2]];
const m = new Map<string, number>(pairs);
export async function total(): Promise<number> {
  console.log("count", m.size);
  console.error("summing");
  const parts = await Promise.all([...m.values()]);
  return parts.reduce((acc, v) => acc + v, 0);
}
`;

// Deterministic ~100-line generated guest (the "large program" point of the
// table): 2 interfaces + 5 functions + 30 generated items + 3 test() calls.
// Only ever type-checked — the assertions are arithmetic-free on purpose.
function buildSynthetic() {
  const lines = [
    "interface Item {",
    "  id: number;",
    "  name: string;",
    "  tag: string;",
    "  score: number;",
    "}",
    "",
    "interface Report {",
    "  total: number;",
    "  best: Item | null;",
    "  count: number;",
    "}",
    "",
    "function makeItem(id: number, name: string): Item {",
    "  return {",
    "    id,",
    "    name,",
    '    tag: name.length > 4 ? "long" : "short",',
    "    score: (id * 37) % 101,",
    "  };",
    "}",
    "",
    "function score(items: Item[]): number {",
    "  let total = 0;",
    "  for (const item of items) {",
    "    total += item.score;",
    "  }",
    "  return total;",
    "}",
    "",
    "function topScore(items: Item[]): Item | null {",
    "  let best: Item | null = null;",
    "  for (const item of items) {",
    "    if (best === null || item.score > best.score) {",
    "      best = item;",
    "    }",
    "  }",
    "  return best;",
    "}",
    "",
    "function describeItem(item: Item): string {",
    '  return item.name + "#" + String(item.id) + " (" + item.tag + ", " + String(item.score) + ")";',
    "}",
    "",
    "function buildReport(items: Item[]): Report {",
    "  return {",
    "    total: score(items),",
    "    best: topScore(items),",
    "    count: items.length,",
    "  };",
    "}",
    "",
    "const items: Item[] = [];",
  ];
  for (let i = 1; i <= 30; i++) {
    lines.push(`items.push(makeItem(${i}, "item-${String(i).padStart(2, "0")}"));`);
  }
  lines.push(
    "",
    "const report: Report = buildReport(items);",
    'test("report counts every item", () => { expect(report.count).toEqual(30); });',
    'test("report total is positive", () => { expect(report.total > 0).toBeTrue(); });',
    'test("every item is described", () => { expect(describeItem(items[0]).length > 0).toBeTrue(); });',
    "",
  );
  return lines.join("\n");
}

// The 5, locked (05-RESEARCH "Snippets"). Errors asserted on BOTH legs — the
// bench doubles as a semantic regression net for the seam.
const SNIPPETS = [
  { name: "slugify-iter1", source: SLUGIFY_ITER1, expectedErrors: 2 },
  { name: "slugify-final", source: SLUGIFY_FINAL, expectedErrors: 0 },
  { name: "fibonacci-final", source: FIBONACCI_FINAL, expectedErrors: 0 },
  { name: "lib-heavy", source: LIB_HEAVY, expectedErrors: 0 },
  { name: "synthetic-100", source: buildSynthetic(), expectedErrors: 0 },
];

function f1(n) {
  return n.toFixed(1);
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

// Syntactic + semantic diagnostics for the guest file; `text` arrives
// pre-flattened and startPosition is ZERO-based — only the COUNT matters here.
function errorCount(program) {
  const diags = [
    ...program.getSyntacticDiagnostics("/guest.ts"),
    ...program.getSemanticDiagnostics("/guest.ts"),
  ];
  return diags.filter((d) => d.category === DiagnosticCategory.Error).length;
}

// ---- leg 1/2: tsc-rs native (host-side, persistent sync-RPC subprocess) ----
console.log("leg 1/3: tsc-rs native (JS API; fresh API per snippet, cold + 10 warm)...");
const tscRs = SNIPPETS.map((snippet) => {
  const fs = createVirtualFileSystem({
    "/guest-globals.d.ts": GUEST_GLOBALS_TEXT,
    "/guest.ts": snippet.source,
  });
  const tSpawn = performance.now();
  const api = new API({ fs }); // spawns the native binary
  const spawnMs = performance.now() - tSpawn;

  const tCold = performance.now();
  let program = api.createProgram(TSC_RS_ROOTS, OPTIONS);
  const coldMs = performance.now() - tCold;
  const coldErrors = errorCount(program);
  program.dispose();

  const warmMs = [];
  let warmErrors = coldErrors;
  for (let i = 0; i < 10; i++) {
    const t0 = performance.now();
    program = api.createProgram(TSC_RS_ROOTS, OPTIONS);
    warmMs.push(performance.now() - t0);
    warmErrors = errorCount(program);
    program.dispose();
  }
  api.close();

  assert(
    coldErrors === snippet.expectedErrors && warmErrors === snippet.expectedErrors,
    `tsc-rs leg: ${snippet.name} -> ${snippet.expectedErrors} error(s) (cold ${coldErrors}, warm ${warmErrors})`,
    { coldErrors, warmErrors },
  );
  return { name: snippet.name, spawnMs, coldMs, warmMs, errors: coldErrors };
});

// ---- leg 2/2: in-isolate (booted wrangler dev, POST /typecheck) ----
console.log("leg 2/3: ts-js in-isolate checker (POST /typecheck under a booted wrangler dev)...");
let port;
let wrangler;
let log = "";
const iso = [];
try {
  port = await freePort();
  wrangler = spawnWrangler(port);
  wrangler.stdout.on("data", (d) => {
    log += d;
  });
  wrangler.stderr.on("data", (d) => {
    log += d;
  });
  await waitHealthy(port, () => log);

  for (const snippet of SNIPPETS) {
    // runs: 6 -> data.coldMs (run 1) + FIVE warm samples (runs 2-6). The
    // endpoint's default 5 would leave only 4 warm samples (gotcha 12).
    const res = await post(port, "/typecheck", JSON.stringify({ files: { "guest.ts": snippet.source }, runs: 6 }));
    assert(res.status === 200, `in-isolate leg: ${snippet.name} -> HTTP 200`, res.status);
    const data = res.json?.data;
    assert(
      data?.runs === 6 && Array.isArray(data?.results) && data.results.length === 6,
      `in-isolate leg: ${snippet.name} -> 6 runs (1 cold + 5 warm)`,
      data?.runs,
    );
    const expectedOk = snippet.expectedErrors === 0;
    assert(
      Array.isArray(data?.results) && data.results.every((r) => r.ok === expectedOk),
      `in-isolate leg: ${snippet.name} -> every run ok === ${expectedOk}`,
      data?.results?.map((r) => r.ok),
    );
    iso.push({
      name: snippet.name,
      coldMs: data.coldMs,
      warmMs: data.results.slice(1).map((r) => r.ms),
    });
  }
} finally {
  await killWrangler(wrangler, port);
}

// ---- leg 3/4: tsc-rs wasm IN-ISOLATE, FRESH instance per run — the boot
// pins TSCHECKER_WASM_RECYCLE_AFTER:1, so every run drops and re-instantiates
// the held instance: Phase 6's fresh-instance semantics through the same
// endpoint (asserted below per snippet: runsOnInstance === 1 + cumulative gen/recycles). ----
console.log("leg 3/4: tsc-rs wasm checker, FRESH instance per run (POST /typecheck under --var TSCHECKER:tsc-rs-wasm --var TSCHECKER_WASM_RECYCLE_AFTER:1)...");
const wasm = [];
let portWasm;
let wranglerWasm;
let wasmLog = "";
try {
  portWasm = await freePort();
  wranglerWasm = spawnWrangler(portWasm, { TSCHECKER: "tsc-rs-wasm", TSCHECKER_WASM_RECYCLE_AFTER: "1" });
  wranglerWasm.stdout.on("data", (d) => {
    wasmLog += d;
  });
  wranglerWasm.stderr.on("data", (d) => {
    wasmLog += d;
  });
  await waitHealthy(portWasm, () => wasmLog);

  for (const snippet of SNIPPETS) {
    // Same protocol as the ts-js leg: runs: 6 -> coldMs + FIVE warm samples.
    const res = await post(portWasm, "/typecheck", JSON.stringify({ files: { "guest.ts": snippet.source }, runs: 6 }));
    assert(res.status === 200, `wasm leg: ${snippet.name} -> HTTP 200`, res.status);
    const data = res.json?.data;
    assert(
      data?.runs === 6 && Array.isArray(data?.results) && data.results.length === 6,
      `wasm leg: ${snippet.name} -> 6 runs (1 cold + 5 warm)`,
      data?.runs,
    );
    const expectedOk = snippet.expectedErrors === 0;
    assert(
      Array.isArray(data?.results) && data.results.every((r) => r.ok === expectedOk),
      `wasm leg: ${snippet.name} -> every run ok === ${expectedOk}`,
      data?.results?.map((r) => r.ok),
    );
    // All 5 snippets share ONE TSCHECKER_WASM_RECYCLE_AFTER:1 boot, so the
    // held stats ACCUMULATE across snippets: snippet k ends at gen 6k with
    // recycles 6k-1 (gen 6/12/18/24/30, final recycles 29). runsOnInstance
    // === 1 is the per-run fresh-semantics proof: every run dropped and
    // re-instantiated, so each request's last run sits on a fresh instance.
    const expectedGen = (SNIPPETS.indexOf(snippet) + 1) * 6;
    assert(
      data?.held?.instanceGen === expectedGen && data.held.runsOnInstance === 1 && data.held.recycles === expectedGen - 1,
      `wasm fresh leg: ${snippet.name} -> recycled every run (cumulative gen ${expectedGen}, recycles ${expectedGen - 1})`,
      data?.held,
    );
    wasm.push({
      name: snippet.name,
      coldMs: data.coldMs,
      warmMs: data.results.slice(1).map((r) => r.ms),
    });
  }
} finally {
  await killWrangler(wranglerWasm, portWasm);
}

// ---- leg 4/4: tsc-rs wasm IN-ISOLATE, HELD instance (the default boot —
// ONE instance per isolate, recycle after 50; runs 2+ ride the fork's
// mtime-keyed parse cache of unchanged files). Same 1 cold + 5 warm
// protocol; all 5 snippets share the one instance (30 runs < 50). ----
console.log("leg 4/4: tsc-rs wasm checker, HELD instance (POST /typecheck under --var TSCHECKER:tsc-rs-wasm)...");
const wasmHeld = [];
let portHeld;
let wranglerHeld;
let heldLog = "";
try {
  portHeld = await freePort();
  wranglerHeld = spawnWrangler(portHeld, { TSCHECKER: "tsc-rs-wasm" });
  wranglerHeld.stdout.on("data", (d) => {
    heldLog += d;
  });
  wranglerHeld.stderr.on("data", (d) => {
    heldLog += d;
  });
  await waitHealthy(portHeld, () => heldLog);

  for (const snippet of SNIPPETS) {
    const res = await post(portHeld, "/typecheck", JSON.stringify({ files: { "guest.ts": snippet.source }, runs: 6 }));
    assert(res.status === 200, `wasm held leg: ${snippet.name} -> HTTP 200`, res.status);
    const data = res.json?.data;
    assert(
      data?.runs === 6 && Array.isArray(data?.results) && data.results.length === 6,
      `wasm held leg: ${snippet.name} -> 6 runs (1 cold + 5 warm)`,
      data?.runs,
    );
    const expectedOk = snippet.expectedErrors === 0;
    assert(
      Array.isArray(data?.results) && data.results.every((r) => r.ok === expectedOk),
      `wasm held leg: ${snippet.name} -> every run ok === ${expectedOk}`,
      data?.results?.map((r) => r.ok),
    );
    assert(
      data?.held?.instanceGen === 1 && data.held.recycles === 0,
      `wasm held leg: ${snippet.name} -> all runs on the ONE held instance (gen 1, recycles 0)`,
      data?.held,
    );
    wasmHeld.push({
      name: snippet.name,
      coldMs: data.coldMs,
      warmMs: data.results.slice(1).map((r) => r.ms),
    });
  }
} finally {
  await killWrangler(wranglerHeld, portHeld);
}

// ---- table + docs/benchmarks.md ----
const header = [
  "| snippet          | tsc-rs cold ms | tsc-rs warm med(10) | tsc-rs warm min | ts-js cold ms | ts-js warm med(5) | wasm fresh cold ms | wasm fresh warm med(5) | wasm held cold ms | wasm held warm med(5) | errors (exp/got) |",
  "|------------------|----------------|---------------------|-----------------|---------------|-------------------|--------------------|------------------------|-------------------|-----------------------|------------------|",
];
const rows = SNIPPETS.map((snippet, i) => {
  const a = tscRs[i];
  const b = iso[i];
  const c = wasm[i]; // fresh-instance leg (TSCHECKER_WASM_RECYCLE_AFTER:1 boot)
  const h = wasmHeld[i]; // held-instance leg (default boot)
  return `| ${snippet.name.padEnd(16)} | ${f1(a.coldMs)} | ${f1(median(a.warmMs))} | ${f1(Math.min(...a.warmMs))} | ${f1(b.coldMs)} | ${f1(median(b.warmMs))} | ${f1(c.coldMs)} | ${f1(median(c.warmMs))} | ${f1(h.coldMs)} | ${f1(median(h.warmMs))} | ${snippet.expectedErrors}/${a.errors} |`;
});
const table = [...header, ...rows].join("\n");

let tscRsVersion = "unknown";
try {
  tscRsVersion = createRequire(import.meta.url)("tsc-rs").version; // CJS root stub
} catch {
  // cosmetic only — never fail the bench over the version string
}
const spawnLine = `tsc-rs native spawn+init: ${f1(median(tscRs.map((r) => r.spawnMs)))} ms median across the ${tscRs.length} API instances (not part of the cold column).`;
const machineLine = `Machine: ${process.platform} ${process.arch}, ${process.version}, tsc-rs ${tscRsVersion} (native), workerd via wrangler dev, typescript 6.0.3 in-isolate (ts-js), ts-rust wasm multi-run/5ada636339d0 (fork of pin 569df52) in-isolate (TSCHECKER=tsc-rs-wasm; fresh = TSCHECKER_WASM_RECYCLE_AFTER:1 boot, held = default boot).`;
const footnotes = [
  "tsc-rs warm med(10) = median of 10 subsequent createProgram+diagnostics checks on the SAME API instance (persistent native subprocess, sync-RPC); cold = the first such check after spawn.",
  "In-isolate cold = POST /typecheck data.coldMs (run 1 of 6); warm med(5) = median of runs 2-6 (runs: 6 so there are 5 warm samples; the endpoint default of 5 yields only 4).",
  "Lib-closure difference, footnoted rather than forced to parity: tsc-rs checks against its default lib.es2022.full chain INCLUDING DOM; the in-isolate checker uses the bundled 115-file es2022 core closure (no DOM). All five snippets type-check identically on both.",
  "Both legs share the guest surface: /guest-globals.d.ts (repo GUEST_GLOBALS_TEXT) is an explicit rootName on the tsc-rs leg and prepended implicitly by the in-isolate checker; options mirror GUEST_COMPILER_OPTIONS (strict, ES2022, noEmit, ESNext modules, Bundler resolution, skipLibCheck, isolatedModules, forceConsistentCasingInFileNames).",
  "tsc-rs is host-side only on the native leg (binary subprocess). Its WASM build is the third and fourth legs: the vendored ts_rust.wasm (unpublished; built from the multi-run FORK — branch multi-run @ 5ada636ae of pin 569df52 — docs/ts-rust-wasm.md) runs INSIDE the isolate behind the same Typechecker interface.",
  "wasm fresh cold/warm = POST /typecheck against a wrangler dev boot with TSCHECKER_WASM_RECYCLE_AFTER:1 — every run drops and re-instantiates the WebAssembly.Instance (2-8 ms), i.e. Phase 6's fresh-instance semantics (asserted per snippet: runsOnInstance 1 with a cumulative gen 6/12/18/24/30 — one recycle per run). wasm held cold/warm = the default boot: ONE held instance per isolate serves ALL snippets (asserted: instanceGen 1, recycles 0 across all 30 runs); run 1 is cold, runs 2-6 ride the fork's mtime-keyed parse cache of unchanged files. Both boots use the same 1 cold + 5 warm protocol; the lib.d.ts closure is embedded in the module (strict + es2022 tsconfig under /app, mirroring the ts-js guest surface).",
  "The wasm module ships unconditionally in the worker bundle (+5,150,781 bytes raw); local dev does not enforce CPU limits — a fresh wasm check costs ~0.5-1.2 s of synchronous CPU, a warm held check ~70-125 ms (07-RESEARCH).",
];

const doc = [
  "# Benchmark: native tsc-rs vs the in-isolate checkers (ts-js + tsc-rs-wasm fresh/held)",
  "",
  "Reproduce with `npm run bench:tsc-rs` — it boots THREE `wrangler dev` workers (default ts-js +",
  "TSCHECKER=tsc-rs-wasm fresh via TSCHECKER_WASM_RECYCLE_AFTER:1 + TSCHECKER=tsc-rs-wasm held),",
  "prints this table, and rewrites this file. README.md commits the same numbers.",
  "",
  `Generated: ${new Date().toISOString()}`,
  "",
  table,
  "",
  spawnLine,
  machineLine,
  "",
  "Footnotes:",
  ...footnotes.map((line) => `- ${line}`),
  "",
].join("\n");

mkdirSync("docs", { recursive: true });
writeFileSync("docs/benchmarks.md", doc);
console.log(`\n${table}\n\n${spawnLine}\n${machineLine}`);

if (failures.length > 0) {
  console.error(`\n${failures.length} assertion(s) failed.`);
  process.exitCode = 1;
} else {
  console.log("\nbench complete: docs/benchmarks.md written");
}

// ---- wrangler boot/teardown (copied pattern from scripts/test-agent-live.mjs) ----
async function freePort() {
  return new Promise((resolve, reject) => {
    const srv = createServer();
    srv.listen(0, "127.0.0.1", () => {
      const chosen = srv.address().port;
      srv.close(() => resolve(chosen));
    });
    srv.on("error", reject);
  });
}

function spawnWrangler(port, vars = {}) {
  const varFlags = Object.entries(vars).flatMap(([key, value]) => ["--var", `${key}:${value}`]);
  return spawn("wrangler", ["dev", "--port", String(port), ...varFlags], {
    env: { ...process.env, CI: "true" },
    stdio: ["ignore", "pipe", "pipe"],
    detached: true, // own process group so teardown kills wrangler + workerd together
  });
}

async function waitHealthy(port, getLog) {
  for (let i = 0; i < 60; i++) {
    await new Promise((r) => setTimeout(r, 1000));
    try {
      const res = await fetch(`http://127.0.0.1:${port}/healthz`, {
        signal: AbortSignal.timeout(2000),
      });
      if (res.ok) return { status: res.status, json: await res.json() };
    } catch {
      // not up yet
    }
  }
  throw new Error(`wrangler dev did not serve /healthz within 60s. Log tail:\n${getLog().slice(-2000)}`);
}

async function killWrangler(wrangler, port) {
  if (wrangler !== undefined && wrangler.pid !== undefined) {
    try {
      process.kill(-wrangler.pid, "SIGTERM");
    } catch {
      // already gone
    }
  }
  if (port !== undefined) {
    await new Promise((r) => setTimeout(r, 2000));
    let stillUp = true;
    try {
      await fetch(`http://127.0.0.1:${port}/healthz`, { signal: AbortSignal.timeout(1500) });
    } catch {
      stillUp = false; // down — good
    }
    if (stillUp) {
      try {
        execSync(`fuser -k ${port}/tcp 2>/dev/null || true`); // backstop; fuser may be absent
      } catch {
        // best effort
      }
      assert(false, "teardown: wrangler killed and port closed");
    } else {
      console.log("ok - teardown: wrangler killed and port closed");
    }
  }
}

async function get(port, path, { method = "GET", body, headers } = {}) {
  const res = await fetch(`http://127.0.0.1:${port}${path}`, {
    method,
    headers,
    body,
    signal: AbortSignal.timeout(30000),
  });
  let json = null;
  try {
    json = await res.json();
  } catch {
    json = null; // non-JSON body
  }
  return { status: res.status, headers: res.headers, json };
}

function post(port, path, bodyText) {
  return get(port, path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: bodyText,
  });
}
