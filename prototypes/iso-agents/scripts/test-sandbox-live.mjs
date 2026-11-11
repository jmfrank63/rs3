// Live in-workerd verification harness for the Phase 3 sandbox
// (`node scripts/test-sandbox-live.mjs`, chained by npm run test:live).
//
// Same transport rationale as scripts/test-typecheck-live.mjs: NO vitest spec
// may pull the worker/sandbox module graph (quickjs-emscripten-core + the
// .wasm sibling would ride the vitest worker transport — Phase 2 lesson), so
// every guardrail assertion runs here against real workerd via `wrangler dev`.
// Boots CI=true wrangler dev on a free port, polls /healthz, then drives
// POST /sandbox/execute through the research failure-mode matrix: green spec,
// console capture, failing assertion, guest throw, while(true) -> timeout
// within deadline+250ms (THE guardrail), deep recursion -> catchable stack
// overflow, memory hog -> out of memory, async source gate, a transpiled-TS
// guest, fresh-context poison checks (SAND-04), route validation, and the
// healthz identity flip. Kills wrangler and exits non-zero on any failure.
import { spawn, execSync } from "node:child_process";
import { createServer } from "node:net";
import ts from "typescript";

const SANDBOX_IDENTITY = "quickjs-wasmfile-release-sync@0.32.0";
const DEADLINE_MS = 2000; // default SANDBOX_TIMEOUT_MS (the harness sets no env overrides)

const failures = [];
function assert(cond, label, detail) {
  if (cond) {
    console.log(`ok - ${label}`);
  } else {
    failures.push(label);
    console.error(`FAIL - ${label}${detail === undefined ? "" : `: ${JSON.stringify(detail)}`}`);
  }
}

// Guest snippets (plain JS — the sandbox input contract is transpiled-JS).
// CASE ORDER IS LOAD-BEARING: the healthz pre-state is asserted before ANY
// execute, and the green spec runs first so the WASM module loads BEFORE the
// while(true) timing measurement.
const GREEN_SPEC = `
test("adds", () => { expect(1 + 1).toEqual(2); });
test("truthy", () => { expect(1 === 1).toBeTrue(); });
test("close", () => { expect(0.1 + 0.2).toBeCloseTo(0.3); });
6 * 7
`;
const CONSOLE_SPEC = `
console.log("answer", 42);
console.warn("heads up", { a: [1, 2] });
console.error("boom-ish", new Error("captured"));
`;
const FAILING_SPEC = `
test("math is broken on purpose", () => { expect(1 + 1).toEqual(3); });
test("still runs", () => { expect(true).toBeTrue(); });
`;
const THROW_SPEC = "throw new Error('boom');\n";
const WHILE_TRUE_SPEC = "while (true) {}\n1";
const RECURSION_SPEC = "function f() { return f(); }\nf();\n";
const MEMORY_SPEC = 'const s = "x".repeat(268435456);\ns.length;\n';
const ASYNC_FN_SPEC = "async function f() { return 1; }\nf();\n";
const PROMISE_SPEC = "new Promise(function (resolve) { resolve(1); });\n";
const POST_POISON_SPEC = 'test("survived", () => { expect(2 + 2).toEqual(4); });\n1';
// v5 H5: fabricated evidence in the OLD guest-writable __tests global must
// be ignored; the host reads the harness's closure-private records.
const FABRICATE_SPEC = 'globalThis.__tests = [{ name: "fabricated", passed: true }];\ntest("real", () => { expect(1).toEqual(1); });\n2';
// v5 H5: harness functions resist replacement (locked descriptors — strict
// guests throw on reassignment instead of feeding the loop fake results).
const REASSIGN_SPEC = "test = function () {};\n1";
// v5 H6: NaN can never pass a structural assertion (was a false pass both ways).
const NAN_SPEC = 'test("nan close", () => { expect(NaN).toBeCloseTo(42); });\ntest("nan equal", () => { expect(NaN).toEqual(null); });\n1';
// v5 H9: BigInt results/console args survive HTTP serialization (was a 500).
const BIGINT_SPEC = "console.log(1n);\n1n";
// v5 H10: a 1 MB console string must retain at most the 8 KiB cap.
const BIG_LOG_SPEC = 'console.log("x".repeat(1000000));\n1';
// Transpiled TypeScript guest (ROADMAP SC5): harness-side ts.transpileModule
// with the same config Phase 2 uses, then executed in the sandbox. The
// in-worker transpile step feeding the loop lands with CORE-02 in Phase 4.
const TS_GUEST = `
const n: number = 6;
function square(x: number): number {
  return x * x;
}
test("square", () => { expect(square(n)).toEqual(36); });
square(n)
`;

let port;

async function get(path, { method = "GET", body, headers } = {}) {
  const res = await fetch(`http://127.0.0.1:${port}${path}`, {
    method,
    headers,
    body,
    signal: AbortSignal.timeout(10000),
  });
  let json = null;
  try {
    json = await res.json();
  } catch {
    json = null; // non-JSON body
  }
  return { status: res.status, headers: res.headers, json };
}

function post(path, bodyText) {
  return get(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: bodyText,
  });
}

let wrangler;
let wranglerLog = "";
try {
  // Free port: bind 0, read the assigned port, release (tiny race, fine for dev).
  port = await new Promise((resolve, reject) => {
    const srv = createServer();
    srv.listen(0, "127.0.0.1", () => {
      const chosen = srv.address().port;
      srv.close(() => resolve(chosen));
    });
    srv.on("error", reject);
  });
  wrangler = spawn("wrangler", ["dev", "--port", String(port)], {
    env: { ...process.env, CI: "true" },
    stdio: ["ignore", "pipe", "pipe"],
    detached: true, // own process group so teardown kills wrangler + workerd together
  });
  wrangler.on("error", (err) => {
    console.error(`FAIL - could not spawn wrangler (run this via npm run test:live): ${err.message}`);
    process.exit(1);
  });
  wrangler.stdout.on("data", (d) => {
    wranglerLog += d;
  });
  wrangler.stderr.on("data", (d) => {
    wranglerLog += d;
  });

  // Poll /healthz until the dev server answers (60s budget).
  let healthz = null;
  for (let i = 0; i < 60 && healthz === null; i++) {
    await new Promise((r) => setTimeout(r, 1000));
    try {
      const res = await fetch(`http://127.0.0.1:${port}/healthz`, {
        signal: AbortSignal.timeout(2000),
      });
      if (res.ok) healthz = { status: res.status, headers: res.headers, json: await res.json() };
    } catch {
      // not up yet
    }
  }
  if (healthz === null) {
    throw new Error(`wrangler dev did not serve /healthz within 60s. Log tail:\n${wranglerLog.slice(-2000)}`);
  }

  const runtime = healthz.json.data.runtime;

  // -- Pre-state: asserted BEFORE any /sandbox/execute (lazy module unloaded) --
  assert(
    runtime.sandbox === "unloaded",
    "healthz: sandbox is unloaded before any execution",
    runtime.sandbox,
  );

  // -- Case 1 (matrix row: happy path): green spec; first execute loads the module --
  const green = await post("/sandbox/execute", JSON.stringify({ code: GREEN_SPEC }));
  assert(green.status === 200 && green.json.ok === true, "green: HTTP 200 ok envelope");
  assert(green.json.data.status === "ok", "green: status ok", green.json.data);
  assert(green.json.data.result === 42, "green: completion value 42", green.json.data.result);
  assert(green.json.data.tests.length === 3, "green: three tests recorded");
  assert(green.json.data.tests.every((t) => t.passed === true), "green: all tests passed");
  assert(green.json.data.ms > 0 && Number.isFinite(green.json.data.ms), "green: ms positive and finite");
  assert(green.json.data.logs.length === 0, "green: no logs for a silent guest");

  // -- Case 2 (matrix row 14): console capture (SAND-03) --
  const con = await post("/sandbox/execute", JSON.stringify({ code: CONSOLE_SPEC }));
  assert(con.json.data.status === "ok", "console: status ok", con.json.data);
  assert(con.json.data.logs.length === 3, "console: three entries captured");
  const l0 = con.json.data.logs[0];
  const l1 = con.json.data.logs[1];
  const l2 = con.json.data.logs[2];
  assert(
    l0.level === "log" && JSON.stringify(l0.args) === JSON.stringify(["answer", 42]),
    "console: log level + primitive args",
    l0,
  );
  assert(
    l1.level === "warn" && JSON.stringify(l1.args) === JSON.stringify(["heads up", { a: [1, 2] }]),
    "console: warn level + object arg",
    l1,
  );
  assert(
    l2.level === "error" &&
      l2.args[1] &&
      l2.args[1].name === "Error" &&
      l2.args[1].message === "captured",
    "console: error level + Error dumped to {name,message}",
    l2,
  );

  // -- Case 3 (verified guest-harness behavior): failing assertion -> {passed:false, error} --
  const failing = await post("/sandbox/execute", JSON.stringify({ code: FAILING_SPEC }));
  assert(failing.json.data.status === "ok", "failing: status ok (guest evaluated fine)");
  assert(failing.json.data.tests.length === 2, "failing: two tests recorded");
  assert(failing.json.data.tests[0].passed === false, "failing: first test failed");
  assert(
    typeof failing.json.data.tests[0].error === "string" &&
      failing.json.data.tests[0].error.includes("expected 3, got 2"),
    "failing: error message carries expected/got",
    failing.json.data.tests[0],
  );
  assert(failing.json.data.tests[1].passed === true, "failing: second test still ran and passed");

  // -- Case 4: guest throw -> status error --
  const thrown = await post("/sandbox/execute", JSON.stringify({ code: THROW_SPEC }));
  assert(thrown.json.data.status === "error", "guest throw: status error");
  assert(
    typeof thrown.json.data.error === "string" && thrown.json.data.error.includes("Error: boom"),
    "guest throw: error carries Error: boom",
    thrown.json.data.error,
  );

  // -- v5 H5: fabricated test evidence in guest globals is ignored --
  const fabricate = await post("/sandbox/execute", JSON.stringify({ code: FABRICATE_SPEC }));
  assert(
    fabricate.status === 200 && fabricate.json.data.status === "ok",
    "H5: fabricating guest still evaluates (status ok)",
    fabricate.json.data,
  );
  assert(
    fabricate.json.data.tests.length === 1 &&
      fabricate.json.data.tests[0].name === "real" &&
      fabricate.json.data.tests[0].passed === true,
    "H5: host reads the PRIVATE records — 0 fabricated passes leak into the results",
    fabricate.json.data.tests,
  );
  const reassign = await post("/sandbox/execute", JSON.stringify({ code: REASSIGN_SPEC }));
  assert(
    reassign.json.data.status === "error" &&
      typeof reassign.json.data.error === "string" &&
      /read-?only/i.test(reassign.json.data.error),
    "H5: reassigning test() throws in strict guests (harness resists replacement)",
    reassign.json.data,
  );

  // -- v5 H6: NaN never passes a structural assertion --
  const nanCase = await post("/sandbox/execute", JSON.stringify({ code: NAN_SPEC }));
  assert(
    nanCase.json.data.status === "ok" &&
      nanCase.json.data.tests.length === 2 &&
      nanCase.json.data.tests.every((t) => t.passed === false),
    "H6: expect(NaN).toBeCloseTo(42) and expect(NaN).toEqual(null) both FAIL (was false-pass x2)",
    nanCase.json.data.tests,
  );

  // -- v5 H9: BigInt result + console args survive HTTP serialization --
  const bigintRun = await post("/sandbox/execute", JSON.stringify({ code: BIGINT_SPEC }));
  assert(
    bigintRun.status === 200 &&
      bigintRun.json.ok === true &&
      bigintRun.json.data.status === "ok" &&
      bigintRun.json.data.result === "1",
    "H9: execute(1n) -> 200 with a stringified result (was a 500)",
    bigintRun.json.data,
  );
  assert(
    bigintRun.json.data.logs.length === 1 &&
      bigintRun.json.data.logs[0].args[0] === "1",
    "H9: console.log(1n) captured as a string (was a guest-visible serialization error)",
    bigintRun.json.data.logs,
  );

  // -- v5 H10: the console cap bounds RETAINED bytes --
  const bigLog = await post("/sandbox/execute", JSON.stringify({ code: BIG_LOG_SPEC }));
  const retainedLen = bigLog.json.data.logs?.[0]?.args?.[0]?.length ?? -1;
  assert(
    bigLog.status === 200 &&
      bigLog.json.data.logs.length === 1 &&
      retainedLen >= 0 &&
      retainedLen <= 8192,
    "H10: console.log of 1MB retains at most the 8192-byte cap (was the full dump)",
    retainedLen,
  );

  // -- Case 5 (matrix rows 1+2, ROADMAP SC1): while(true) -> structured timeout --
  const wtStart = Date.now();
  const wt = await post("/sandbox/execute", JSON.stringify({ code: WHILE_TRUE_SPEC }));
  const wall = Date.now() - wtStart;
  assert(
    wt.status === 200 && wt.json.ok === true && wt.json.data.status === "timeout",
    "while-true: structured timeout result (wrangler dev never hangs)",
    wt.json.data,
  );
  assert(wall < DEADLINE_MS + 250, `while-true: wall ${wall}ms within deadline+250ms`, wall);
  assert(
    typeof wt.json.data.error === "string" && wt.json.data.error.includes("deadline"),
    "while-true: error mentions the deadline",
    wt.json.data.error,
  );
  const afterTimeout = await post("/sandbox/execute", JSON.stringify({ code: POST_POISON_SPEC }));
  assert(
    afterTimeout.json.data.status === "ok" &&
      afterTimeout.json.data.tests.length === 1 &&
      afterTimeout.json.data.tests[0].passed === true,
    "SAND-04: fresh context after a timed-out guest",
    afterTimeout.json.data,
  );

  // -- Case 6 (matrix row 7, SC2): deep recursion -> catchable stack overflow (64KiB) --
  const rec = await post("/sandbox/execute", JSON.stringify({ code: RECURSION_SPEC }));
  assert(
    rec.json.data.status === "error",
    "deep recursion: status error (catchable at 64KiB stack)",
  );
  assert(
    typeof rec.json.data.error === "string" && rec.json.data.error.includes("stack overflow"),
    "deep recursion: error contains stack overflow",
    rec.json.data.error,
  );
  const afterRecursion = await post("/sandbox/execute", JSON.stringify({ code: POST_POISON_SPEC }));
  assert(
    afterRecursion.json.data.status === "ok" && afterRecursion.json.data.tests[0].passed === true,
    "SAND-04: fresh context after a stack-overflowing guest",
    afterRecursion.json.data,
  );

  // -- Case 7 (matrix row 6, SC2): memory hog -> catchable out of memory --
  // Single huge alloc (256M chars vs 64MiB limit) fails instantly — deterministic
  // "error", never a timeout (churn loops run ~3s and would race the deadline).
  const mem = await post("/sandbox/execute", JSON.stringify({ code: MEMORY_SPEC }));
  assert(mem.json.data.status === "error", "memory hog: status error");
  assert(
    typeof mem.json.data.error === "string" && mem.json.data.error.includes("out of memory"),
    "memory hog: catchable out of memory",
    mem.json.data.error,
  );

  // -- Case 8 (matrix row 12): async source gate --
  const asyncFn = await post("/sandbox/execute", JSON.stringify({ code: ASYNC_FN_SPEC }));
  assert(
    asyncFn.json.data.status === "error" &&
      typeof asyncFn.json.data.error === "string" &&
      asyncFn.json.data.error.includes("async execution is not supported"),
    "async gate: async function source rejected with clear message",
    asyncFn.json.data,
  );
  const promiseSrc = await post("/sandbox/execute", JSON.stringify({ code: PROMISE_SPEC }));
  assert(
    promiseSrc.json.data.status === "error" &&
      promiseSrc.json.data.error.includes('source contains "Promise"'),
    "async gate: Promise source rejected",
    promiseSrc.json.data,
  );

  // -- Case 9 (ROADMAP SC5): transpiled TS guest executes end-to-end --
  const transpiled = ts.transpileModule(TS_GUEST, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, isolatedModules: true },
  }).outputText;
  const tsGuest = await post("/sandbox/execute", JSON.stringify({ code: transpiled }));
  assert(
    tsGuest.json.data.status === "ok",
    "transpiled TS guest: status ok (ts.transpileModule -> sandbox)",
    tsGuest.json.data,
  );
  assert(
    tsGuest.json.data.tests.length === 1 && tsGuest.json.data.tests[0].passed === true,
    "transpiled TS guest: assertion passes",
  );
  assert(
    tsGuest.json.data.result === 36,
    "transpiled TS guest: completion value 36",
    tsGuest.json.data.result,
  );

  // -- Case 10: route validation --
  const noCode = await post("/sandbox/execute", JSON.stringify({}));
  assert(noCode.status === 400 && noCode.json.error.code === "bad_request", "missing code: 400 bad_request");
  const malformed = await post("/sandbox/execute", "{nope");
  assert(malformed.status === 400 && malformed.json.error.code === "bad_request", "malformed JSON: 400 bad_request");
  const notAllowed = await get("/sandbox/execute");
  assert(notAllowed.status === 405, "GET /sandbox/execute: 405 status");
  assert(
    notAllowed.headers.get("allow") === "POST",
    "GET /sandbox/execute: allow: POST response header",
  );

  // -- Case 11: identity flip --
  const flip = await get("/healthz");
  assert(
    flip.status === 200 && flip.json.data.runtime.sandbox === SANDBOX_IDENTITY,
    "identity flip: sandbox identity after first execution",
    flip.json.data.runtime.sandbox,
  );
} finally {
  // Teardown: kill the whole process group (NEVER `pkill -f wrangler` — argv
  // self-match kills your own shell), then a port-scoped backstop.
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

if (failures.length > 0) {
  console.error(`\n${failures.length} assertion(s) failed. wrangler log tail:\n${wranglerLog.slice(-2000)}`);
  process.exitCode = 1;
}
