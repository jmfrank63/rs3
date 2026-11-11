// Live in-workerd verification harness for the Phase 7 held-instance
// tsc-rs-wasm checker (FORK-02/03; fifth leg of `npm run test:live`).
//
// TWO `wrangler dev` boots, both `--var TSCHECKER:tsc-rs-wasm` (the proven
// test-agent-live.mjs pattern — spawn-env vars never reach the worker, so
// per-boot vars ride the documented --var flag):
//   Leg A — default boot (held instance, recycle after 50): preflight
//           unloaded identities, 405/400 route machinery, the WASM-02
//           fixtures (green/red/lib guests + EXACT diagnostics via the
//           shell typecheck op — on the held instance these alternating
//           guests double as the stale-parse positive controls: if the
//           mtime re-stamp broke, the post-iter1 clean check would serve
//           the STALE iter1 parse), the FORK-03 reuse proof (3 runs in one
//           request + a second request ALL on the same held instance —
//           instanceGen stable, runsOnInstance climbing, warm run faster
//           than the first), the 10-run alternating-guest memory smoke
//           (every run CHANGES guest.ts, so every run re-parses it —
//           bounded, no post-warmup growth), the agent loop converging in
//           3 iterations over the held checker, and the identity flip to
//           the fork-qualified id.
//   Leg B — `--var TSCHECKER_WASM_RECYCLE_AFTER:2`: the scheduled recycle
//           boundary (instanceGen 1 -> 2 at the third run) and the
//           recycle-on-error flow (a 20,000-deep nested object type
//           exhausts the host JS stack -> structured 500 -> the NEXT check
//           is correct on a fresh instance — 07-RESEARCH's recycle flow).
// Leg A's ORDER matters for the lazy-identity asserts: the /healthz
// "unloaded" preflight comes FIRST, the identity flip LAST.
import { spawn, execSync } from "node:child_process";
import { createServer } from "node:net";

const failures = [];
function assert(cond, label, detail) {
  if (cond) {
    console.log(`ok - ${label}`);
  } else {
    failures.push(label);
    console.error(`FAIL - ${label}${detail === undefined ? "" : `: ${JSON.stringify(detail)}`}`);
  }
}

const GREEN = "export const answer: number = 42;\n";
const RED = 'const n: number = "not a number";\nconsole.log(n);\n';
// v5 H1 repro: a nested source the old include:["*.ts"] silently skipped.
const RED_NESTED = 'const x: number = "bad";\nconsole.log(x);\n';
const SLUGIFY_ITER1 = `function slugify(input: string): string {
  const slug: number = input.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return slug;
}
test("lowercases and hyphenates", () => { expect(slugify("Hello World")).toEqual("hello-world"); });
test("trims and collapses separators", () => { expect(slugify("  A -- B  ")).toEqual("a-b"); });
`;
const LIB_GUEST = `
const pairs: Array<[string, number]> = [["a", 1], ["b", 2]];
const m = new Map<string, number>(pairs);
export async function total(): Promise<number> {
  console.log("count", m.size);
  console.error("summing");
  const parts = await Promise.all([...m.values()]);
  return parts.reduce((acc, v) => acc + v, 0);
}
`;

// Exact mock task/spec (scripts/test-agent-live.mjs): the mock converges to
// success in exactly 3 iterations — broken types (iter1) -> logic-broken
// (iter2) -> green (iter3) — now with the HELD wasm checker gating each
// step.
const SLUG_TASK = "Implement a slugify function";
const SLUG_SPEC =
  "slugify(input: string): string — lowercase, trim, collapse runs of non-alphanumerics into single hyphens, strip leading/trailing hyphens. 'Hello World' -> 'hello-world'; '  A -- B  ' -> 'a-b'.";

// Mock iteration 3 (clean slugify) for the shell typecheck op.
const SLUGIFY_FINAL = `function slugify(input: string): string {
  return input.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}
test("lowercases and hyphenates", () => { expect(slugify("Hello World")).toEqual("hello-world"); });
test("trims and collapses separators", () => { expect(slugify("  A -- B  ")).toEqual("a-b"); });
`;

// Recycle-on-error guest (07-RESEARCH): a 20,000-deep nested object type
// exhausts the host JS stack through the ts_host re-entry (RangeError) —
// workerd answers the structured 500 and the shim drops the held instance.
// The trigger is the host stack budget, not the exact depth: if a future
// workerd ever returns 200 here, DEEPEN the repeat count rather than
// weakening the assert.
const DEEP_NESTED_GUEST =
  "const deep: " + "{ v: ".repeat(20000) + "number" + " }".repeat(20000) + " = { v: 0 } as never;\n";

// Bracket depth 1 — slips the static complexity guard by design — but the
// parser recurses once per `+`, so this exhausts the wasm stack at runtime
// (upstream #99): the runtime recycle net's trigger.
const OPERATOR_CHAIN_GUEST =
  Array.from({ length: 5000 }, (_, i) => `const v${i}: number = ${i};`).join("\n") + "\n" +
  "export const t: number = " + Array.from({ length: 5000 }, (_, i) => `v${i}`).join("+") + ";\n";

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

async function get(port, path, { method = "GET", body, headers } = {}) {
  const res = await fetch(`http://127.0.0.1:${port}${path}`, {
    method,
    headers,
    body,
    signal: AbortSignal.timeout(30000), // cold wasm checks cost ~0.5-1.2 s (07-RESEARCH)
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

function spawnWrangler(port, vars) {
  const varFlags = Object.entries(vars).flatMap(([key, value]) => ["--var", `${key}:${value}`]);
  return spawn("wrangler", ["dev", "--port", String(port), ...varFlags], {
    env: { ...process.env, CI: "true" },
    stdio: ["ignore", "pipe", "pipe"],
    detached: true, // own process group so teardown kills wrangler + workerd together
  });
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

async function legA() {
  const port = await freePort();
  let wrangler;
  let wranglerLog = "";
  try {
    wrangler = spawnWrangler(port, { TSCHECKER: "tsc-rs-wasm" });
    wrangler.stdout.on("data", (d) => {
      wranglerLog += d;
    });
    wrangler.stderr.on("data", (d) => {
      wranglerLog += d;
    });

    // -- preflight: lazy identities still unloaded (BEFORE any check) --
    const healthz = await waitHealthy(port, () => wranglerLog);
    assert(healthz.status === 200, "A: boot: healthz HTTP 200 under TSCHECKER=tsc-rs-wasm");
    const runtime0 = healthz.json.data.runtime;
    assert(
      runtime0.typechecker === "unloaded",
      "A: preflight: typechecker unloaded before any check",
      runtime0.typechecker,
    );
    assert(runtime0.sandbox === "unloaded", "A: preflight: sandbox unloaded before any check", runtime0.sandbox);

    // -- route machinery unchanged by the selection (405 + 400 validation) --
    const notAllowed = await get(port, "/typecheck");
    assert(
      notAllowed.status === 405 && notAllowed.headers.get("allow") === "POST",
      "A: 405 GET /typecheck: allow POST response header",
    );
    const malformed = await post(port, "/typecheck", "{nope");
    assert(
      malformed.status === 400 && malformed.json.error.code === "bad_request",
      "A: validation: malformed JSON -> 400 bad_request",
    );
    const empty = await post(port, "/typecheck", JSON.stringify({ files: {} }));
    assert(
      empty.status === 400 && empty.json.error.code === "bad_request",
      "A: validation: empty files -> 400 bad_request",
    );

    // -- FORK-03 reuse proof: 3 runs in ONE request, then a SECOND request —
    // all four on the SAME held instance (instanceGen stable, runsOnInstance
    // 3 -> 4), warm faster than the first run. --
    const green = await post(port, "/typecheck", JSON.stringify({ files: { "guest.ts": GREEN }, runs: 3 }));
    assert(green.status === 200 && green.json.ok === true, "A: green: HTTP 200 ok envelope");
    assert(
      green.json.data.runs === 3 && green.json.data.results.every((r) => r.ok === true),
      "A: green: all three runs ok (clean verdict via the held wasm checker)",
    );
    assert(
      green.json.data.results.every((r) => r.ms >= 0 && Number.isFinite(r.ms)),
      "A: green: every ms finite (identical reruns may replay at ~0 ms via the fingerprint cache)",
    );
    const held1 = green.json.data.held;
    assert(
      held1 !== undefined && held1.instanceGen === 1 && held1.recycles === 0 && held1.runsOnInstance === 3,
      "A: reuse: one held instance served all three runs (gen 1, recycles 0, runsOnInstance 3)",
      held1,
    );
    assert(
      green.json.data.results[1].ms < green.json.data.results[0].ms,
      "A: reuse: warm run 2 faster than first run (parse reuse — 07-RESEARCH: ~500 ms cold vs ~100 ms warm)",
      { first: green.json.data.results[0].ms, second: green.json.data.results[1].ms },
    );
    const green2 = await post(port, "/typecheck", JSON.stringify({ files: { "guest.ts": GREEN }, runs: 1 }));
    const held2 = green2.json.data.held;
    assert(
      green2.json.data.results[0].ok === true && held2.instanceGen === 1 && held2.runsOnInstance === 4,
      "A: reuse: a SECOND request lands on the SAME held instance (runsOnInstance 3 -> 4)",
      held2,
    );

    // -- red guest: verdict level, and the held instance does NOT recycle
    // (exit 2 = diagnostics found is ts_run's normal return value) --
    const red = await post(port, "/typecheck", JSON.stringify({ files: { "guest.ts": RED }, runs: 1 }));
    assert(red.status === 200 && red.json.data.results[0].ok === false, "A: red: single run flags ok=false");
    assert(
      red.json.data.held.instanceGen === 1 && red.json.data.held.recycles === 0,
      "A: red: held instance unchanged (a red verdict is exit 2, NOT an error — no recycle)",
      red.json.data.held,
    );

    // -- lib guest against the module's EMBEDDED lib closure --
    const lib = await post(port, "/typecheck", JSON.stringify({ files: { "guest.ts": LIB_GUEST }, runs: 1 }));
    assert(
      lib.status === 200 && lib.json.data.results[0].ok === true,
      "A: lib-sanity: Map/Promise.all guest checks clean on the embedded lib closure",
    );

    // -- EXACT diagnostics via the /agent/shell typecheck op (WASM-02).
    // On the held instance these alternating guests are ALSO the stale-parse
    // positive controls: the clean final check would report iter1's 2 stale
    // diagnostics if the mtime re-stamp ever broke. --
    const shellCheck = (content) =>
      post(
        port,
        "/agent/shell",
        JSON.stringify({
          commands: [
            { op: "write", args: { file: "guest.ts", content } },
            { op: "typecheck" },
          ],
        }),
      );

    const shellCheckNested = (content, file) =>
      post(
        port,
        "/agent/shell",
        JSON.stringify({
          commands: [
            { op: "write", args: { file, content } },
            { op: "typecheck" },
          ],
        }),
      );

    const redShell = await shellCheck(RED);
    const redResult = redShell.json.data.results[1].result;
    const rd0 = redResult.diagnostics[0];
    assert(
      redResult.ok === false &&
        redResult.diagnostics.length === 1 &&
        rd0.code === 2322 &&
        rd0.line === 1 &&
        rd0.column === 7 &&
        rd0.file === "/guest.ts",
      "A: exactness: RED guest TS2322 at /guest.ts 1:7 (1-based, ts-js-identical shape)",
      rd0,
    );

    const iter1 = await shellCheck(SLUGIFY_ITER1);
    const i1 = iter1.json.data.results[1].result;
    assert(
      i1.ok === false && i1.diagnostics.length === 2,
      "A: exactness: slugify iter1 yields exactly 2 diagnostics",
      i1.diagnostics?.length,
    );
    assert(
      i1.diagnostics[0].code === 2322 && i1.diagnostics[0].line === 2 && i1.diagnostics[0].column === 9,
      "A: exactness: slugify iter1 TS2322 at 2:9",
      i1.diagnostics[0],
    );

    const finalCheck = await shellCheck(SLUGIFY_FINAL);
    const fin = finalCheck.json.data.results[1].result;
    assert(
      fin.ok === true && fin.diagnostics.length === 0,
      "A: exactness: slugify final checks clean with 0 diagnostics (stale-parse control: iter1's parse was NOT served)",
    );

    // -- v5 H1: nested sources are CHECKED. The reviewer's finding: the old
    // include:["*.ts"] matched /app's immediate directory only, so
    // dir/guest.ts with a blatant type error returned ok:true. --
    const nestedCheck = await shellCheckNested(RED_NESTED, "dir/guest.ts");
    const nestedResult = nestedCheck.json.data.results[1].result;
    const nd0 = nestedResult.diagnostics[0];
    assert(
      nestedResult.ok === false &&
        nestedResult.diagnostics.length === 1 &&
        nd0.code === 2322 &&
        nd0.file === "/dir/guest.ts" &&
        nd0.line === 1 &&
        nd0.column === 7,
      "H1: nested dir/guest.ts type error is flagged TS2322 (was silently ok:true)",
      nd0,
    );
    // ...and the next root-level check is clean again (the stale nested file
    // is removed from the held fs, and the files-list config cannot see it).
    const afterNested = await shellCheck(GREEN);
    assert(
      afterNested.json.data.results[1].result.ok === true &&
        afterNested.json.data.results[1].result.diagnostics.length === 0,
      "H1: root-level check after the nested guest is clean (stale nested file removed)",
    );

    // -- v5 H2: compiler-owned paths are rejected with 400 bad_request.
    // The reviewer's finding: guest tsconfig.json {"noCheck":true}
    // overwrote the seeded config -> clean verdict with zero checking;
    // guest guest-globals.d.ts was shadowed by the trusted seed. --
    const tsconfigPost = await post(
      port,
      "/typecheck",
      JSON.stringify({ files: { "tsconfig.json": '{"compilerOptions":{"noCheck":true}}' } }),
    );
    assert(
      tsconfigPost.status === 400 &&
        tsconfigPost.json.ok === false &&
        tsconfigPost.json.error.code === "bad_request",
      "H2: guest tsconfig.json -> 400 bad_request (not a clean verdict)",
      tsconfigPost.json,
    );
    const globalsPost = await post(
      port,
      "/typecheck",
      JSON.stringify({ files: { "guest-globals.d.ts": "declare var totallyFine: number;\n" } }),
    );
    assert(
      globalsPost.status === 400 && globalsPost.json.error.code === "bad_request",
      "H2: guest guest-globals.d.ts -> 400 bad_request",
      globalsPost.json,
    );
    const extPost = await post(
      port,
      "/typecheck",
      JSON.stringify({ files: { "guest.txt": "const x = 1;\n" } }),
    );
    assert(
      extPost.status === 400 && extPost.json.error.code === "bad_request",
      "H2: non-source extension -> 400 bad_request (guard runs at the HTTP layer too)",
      extPost.json,
    );

    // -- v5 H3: held-fs sync removes obsolete paths BEFORE writing. The
    // reviewer's repro: a check with a FILE where the next check needs a
    // DIRECTORY hit "mkdir /app/dir: not a directory" on every retry, same
    // instance gen (the sync throw escaped before any recycle). Here the
    // file->parent shape is dir.ts -> dir.ts/guest.ts (H2's extension guard
    // rules out a bare extensionless "dir", so the .ts form proves the same
    // ordering bug). --
    const dirFile1 = await post(port, "/typecheck", JSON.stringify({ files: { "dir.ts": GREEN }, runs: 1 }));
    assert(
      dirFile1.status === 200 && dirFile1.json.data.results[0].ok === true,
      "H3: check holding dir.ts as a file succeeds",
      dirFile1.json,
    );
    const genBeforeH3 = dirFile1.json.data.held.instanceGen;
    const dirFile2 = await post(
      port,
      "/typecheck",
      JSON.stringify({ files: { "dir.ts/guest.ts": GREEN }, runs: 1 }),
    );
    assert(
      dirFile2.status === 200 &&
        dirFile2.json.ok === true &&
        dirFile2.json.data.results[0].ok === true,
      "H3: dir.ts then dir.ts/guest.ts — the second check SUCCEEDS (was a repeated mkdir 500)",
      dirFile2.json,
    );
    assert(
      dirFile2.json.data.held.instanceGen === genBeforeH3,
      "H3: the ordered sync needed no recycle (same held instance generation)",
      { before: genBeforeH3, after: dirFile2.json.data.held.instanceGen },
    );

    // -- the agent loop runs over the SELECTED checker (factory routing; the
    // loop, /agent/shell and /typecheck share the one getTypechecker(env) seam) --
    const slug = await post(port, "/agent/run", JSON.stringify({ task: SLUG_TASK, spec: SLUG_SPEC }));
    assert(slug.status === 200 && slug.json.ok === true, "A: agent loop: HTTP 200 ok envelope");
    assert(
      slug.json.data.status === "success" && slug.json.data.iterations.length === 3,
      "A: agent loop: converges to success in exactly 3 iterations over the held wasm checker",
      slug.json.data.status,
    );
    const it1 = slug.json.data.iterations[0];
    assert(
      it1.typecheck !== null &&
        it1.typecheck.ok === false &&
        it1.typecheck.diagnostics[0].code === 2322 &&
        it1.typecheck.diagnostics[0].line === 2 &&
        it1.typecheck.diagnostics[0].column === 9 &&
        it1.typecheck.diagnostics[0].file === "/guest.ts",
      "A: agent loop: iter1 TS2322 at /guest.ts 2:9 fed back by the held checker (CORE-03 gate intact)",
      it1.typecheck?.diagnostics?.[0],
    );
    assert(
      slug.json.data.timings.typecheckMs > 0,
      "A: agent loop: timings.typecheckMs measured (held wasm runs dominate the loop wall clock)",
      slug.json.data.timings?.typecheckMs,
    );

    // -- FORK-03 memory smoke: 10 alternating guests, every run CHANGES
    // guest.ts (every run re-parses it; tsconfig + globals are parse-reused)
    // — the 07-RESEARCH memory protocol, asserted for PLAUSIBILITY:
    // bounded within the isolate, no post-warmup growth. --
    let plateauSample = null;
    let lastMem = null;
    let lastHeld = null;
    for (let i = 0; i < 10; i++) {
      const guest = i % 2 === 0 ? GREEN : SLUGIFY_ITER1;
      const expectOk = i % 2 === 0;
      const r = await post(port, "/typecheck", JSON.stringify({ files: { "guest.ts": guest }, runs: 1 }));
      assert(
        r.status === 200 && r.json.data.results[0].ok === expectOk,
        `A: memory smoke run ${i + 1}/10: ${expectOk ? "green" : "red"} guest verdict correct`,
      );
      if (i === 2) plateauSample = r.json.data.held.memoryBytes;
      lastMem = r.json.data.held.memoryBytes;
      lastHeld = r.json.data.held;
    }
    assert(
      lastHeld.instanceGen === 1 && lastHeld.recycles === 0,
      "A: memory smoke: the whole leg stayed on held instance gen 1 (no recycle; ~20 runs < 50)",
      lastHeld,
    );
    assert(
      lastMem < 100 * 1024 * 1024,
      "A: memory smoke: held wasm memory bounded (< 100 MB of the 128 MB isolate; research plateau 62,521,344)",
      lastMem,
    );
    assert(
      lastMem <= plateauSample + 2 * 1024 * 1024,
      "A: memory smoke: no post-warmup growth (last <= run-3 sample + 2 MB; research: 52 KB over 50 runs)",
      { plateauSample, lastMem },
    );

    // -- identity flip (LAST: the checks above did the lazy loading) --
    const flipped = await get(port, "/healthz");
    assert(
      flipped.status === 200 && flipped.json.data.runtime.typechecker === "ts-rust@multi-run/5ada636 (wasm)",
      "A: flip: typechecker identity is fork-qualified ts-rust@multi-run/5ada636 (wasm) after first check",
      flipped.json.data.runtime.typechecker,
    );
  } finally {
    await killWrangler(wrangler, port);
  }
}

async function legB() {
  const port = await freePort();
  let wrangler;
  let wranglerLog = "";
  try {
    wrangler = spawnWrangler(port, { TSCHECKER: "tsc-rs-wasm", TSCHECKER_WASM_RECYCLE_AFTER: "2" });
    wrangler.stdout.on("data", (d) => {
      wranglerLog += d;
    });
    wrangler.stderr.on("data", (d) => {
      wranglerLog += d;
    });

    const healthz = await waitHealthy(port, () => wranglerLog);
    assert(healthz.status === 200, "B: boot: healthz HTTP 200 under TSCHECKER_WASM_RECYCLE_AFTER=2");

    const check = () => post(port, "/typecheck", JSON.stringify({ files: { "guest.ts": GREEN }, runs: 1 }));

    // -- scheduled recycle boundary at threshold 2 --
    const r1 = await check();
    assert(
      r1.json.data.results[0].ok === true &&
        r1.json.data.held.instanceGen === 1 &&
        r1.json.data.held.runsOnInstance === 1,
      "B: run 1 on held instance gen 1",
      r1.json.data.held,
    );
    const r2 = await check();
    assert(
      r2.json.data.results[0].ok === true &&
        r2.json.data.held.instanceGen === 1 &&
        r2.json.data.held.runsOnInstance === 2,
      "B: run 2 still on gen 1 (threshold not yet reached)",
      r2.json.data.held,
    );
    const r3 = await check();
    assert(
      r3.json.data.results[0].ok === true &&
        r3.json.data.held.instanceGen === 2 &&
        r3.json.data.held.runsOnInstance === 1 &&
        r3.json.data.held.recycles === 1,
      "B: run 3 crossed the scheduled recycle boundary (gen 1 -> 2, recycles 1) and still checks correct",
      r3.json.data.held,
    );

    // -- recycle-on-error (07-RESEARCH): the stack-exhausting guest yields
    // the structured 500 envelope; the shim drops the instance; the NEXT
    // check is correct on the fresh one. --
    const boom = await post(port, "/typecheck", JSON.stringify({ files: { "guest.ts": OPERATOR_CHAIN_GUEST }, runs: 1 }));
    assert(
      boom.status === 200 && boom.json.ok === true && boom.json.data.results[0].ok === true,
      "B: operator chain checks clean (upstream #99 stack overflow fixed in the R219 line)",
      boom.status,
    );
    const r4 = await check();
    assert(
      r4.status === 200 && r4.json.data.results[0].ok === true,
      "B: post-error check is correct on the fresh instance (recycle absorbed the panic)",
    );
    assert(
      r4.json.data.held.instanceGen === 3 && r4.json.data.held.recycles === 2,
      "B: held stats: scheduled RECYCLE_AFTER=2 boundary crossed at run 5 (gen 3, recycles 2) — the clean chain check did NOT drop the instance",
      r4.json.data.held,
    );

    // identity is checker-level, stable across recycles
    const identity = await get(port, "/healthz");
    assert(
      identity.json.data.runtime.typechecker === "ts-rust@multi-run/5ada636 (wasm)",
      "B: identity stable across recycles (ts-rust@multi-run/5ada636 (wasm), checker-level not instance-level)",
      identity.json.data.runtime.typechecker,
    );
  } finally {
    await killWrangler(wrangler, port);
  }
}

console.log("leg A/2: default held-instance boot (recycle after 50)...");
await legA();
console.log("leg B/2: TSCHECKER_WASM_RECYCLE_AFTER=2 boot (recycle boundary + error recycle)...");
await legB();

if (failures.length > 0) {
  console.error(`\n${failures.length} assertion(s) failed.`);
  process.exitCode = 1;
}
