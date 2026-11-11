// Live in-workerd verification harness for the Phase 4 agent loop
// (third leg of `npm run test:live`).
//
// Three sequential `wrangler dev` boots (the proven duplicated-harness pattern):
//   Boot A — no env overrides: healthz agent key (mock identity, LAST key),
//     slugify mock run proving the FULL gate sequence typecheck-fail ->
//     sandbox-fail -> success in exactly 3 iterations with per-step timings,
//     the TS2322 exactness re-homed from the Phase 2 harness (iter1:
//     /guest.ts:2:9, 2 diagnostics), GET /agent/run/:id roundtrip, fibonacci
//     + generic-fallback runs, request-level maxIterations=1 cap, validation
//     400s, Phase 1 route regressions, /typecheck stats sanity.
//   Boot B — MOCK_FAIL_AFTER=1 only (LLM_PROVIDER unset -> the mock provider,
//     so the G6 cap gate fires): capped mock run -> max_iterations.
//   Boot C — LLM_PROVIDER=openai pointed at an in-process black-hole server:
//     black-holed openai run -> llm_error (fetch + AbortSignal.timeout proven
//     under the REAL no_nodejs_compat config — closes the research probe
//     caveat), healthz identity flip.
//
// Boot env note: wrangler dev does NOT inherit worker bindings from the OS
// environment (verified live: spawn-env vars never reached the worker), so
// per-boot vars ride the documented `--var KEY:VALUE` flag instead (wrangler
// splits on the FIRST colon, so URL values with ports survive intact).
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

const SLUG_TASK = "Implement a slugify function";
const SLUG_SPEC =
  "slugify(input: string): string — lowercase, trim, collapse runs of non-alphanumerics into single hyphens, strip leading/trailing hyphens. 'Hello World' -> 'hello-world'; '  A -- B  ' -> 'a-b'.";
const FIB_TASK = "Implement a fibonacci function";
const FIB_SPEC = "fibonacci(n: number): number — fib(0)=0, fib(1)=1, fib(7)=13.";
const GREEN_TS = "export const answer: number = 42;\n";
const RED_TS = 'const n: number = "not a number";\nconsole.log(n);\n';

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
  const wrangler = spawn("wrangler", ["dev", "--port", String(port), ...varFlags], {
    env: { ...process.env, CI: "true" },
    stdio: ["ignore", "pipe", "pipe"],
    detached: true, // own process group so teardown kills wrangler + workerd together
  });
  return wrangler;
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

// ---- Boot A: default env (mock provider) ----
let portA;
let wranglerA;
let logA = "";
try {
  portA = await freePort();
  wranglerA = spawnWrangler(portA, {});
  wranglerA.stdout.on("data", (d) => {
    logA += d;
  });
  wranglerA.stderr.on("data", (d) => {
    logA += d;
  });

  const healthz = await waitHealthy(portA, () => logA);
  assert(healthz.status === 200, "boot A: healthz HTTP 200");
  const runtime = healthz.json.data.runtime;
  assert(runtime.agent === "mock-provider ready", "boot A: agent identity is mock-provider ready", runtime.agent);
  const keys = Object.keys(runtime);
  assert(keys[keys.length - 1] === "agent", "boot A: agent is the last runtime key", keys);

  // -- Slugify: THE demonstration (CORE-01/02/03/05, OBS-01) --
  const slug = await post(portA, "/agent/run", JSON.stringify({ task: SLUG_TASK, spec: SLUG_SPEC }));
  assert(slug.status === 200 && slug.json.ok === true, "slugify run: HTTP 200 ok envelope");
  const run = slug.json.data;
  assert(run.status === "success", "slugify run: status success", run.status);
  assert(run.iterations.length === 3, "slugify run: exactly 3 iterations", run.iterations.length);

  const it1 = run.iterations[0];
  const it2 = run.iterations[1];
  const it3 = run.iterations[2];
  assert(it1.typecheck !== null && it1.typecheck.ok === false, "iter1: typecheck failed");
  assert(it1.sandbox === null, "iter1: sandbox skipped (CORE-03 gate)");
  assert(it1.typecheck.diagnostics.length === 2, "iter1: exactly 2 diagnostics", it1.typecheck.diagnostics.length);
  const d0 = it1.typecheck.diagnostics[0];
  assert(d0.code === 2322, "iter1: TS2322 re-homed from the Phase 2 harness (code)", d0);
  assert(d0.line === 2 && d0.column === 9, "iter1: TS2322 at line 2 column 9", d0);
  assert(d0.file === "/guest.ts", "iter1: diagnostic file is /guest.ts", d0.file);
  assert(typeof it1.feedback === "string" && it1.feedback.includes("TS2322"), "iter1: feedback carries the diagnostics");
  assert(it1.transpileMs === null, "iter1: transpile skipped (transpileMs null)");

  assert(it2.typecheck !== null && it2.typecheck.ok === true, "iter2: typecheck ok");
  assert(it2.sandbox !== null && it2.sandbox.status === "ok", "iter2: sandbox executed with status ok");
  const failed2 = it2.sandbox.tests.filter((t) => t.passed === false);
  const passed2 = it2.sandbox.tests.filter((t) => t.passed === true);
  assert(failed2.length === 1 && passed2.length === 1, "iter2: exactly one test failed, one passed", it2.sandbox.tests);
  assert(
    failed2.length === 1 &&
      typeof failed2[0].error === "string" &&
      failed2[0].error.includes('expected "a-b", got "-a-b-"'),
    'iter2: failed test error expected "a-b", got "-a-b-"',
    failed2[0],
  );
  assert(JSON.stringify(it2.files) !== JSON.stringify(it1.files), "iter2: files rewritten (differ from iteration 1)");

  assert(
    it3.sandbox !== null && it3.sandbox.tests.length === 2 && it3.sandbox.tests.every((t) => t.passed === true),
    "iter3: all 2 tests passed",
  );
  assert(it3.files["guest.ts"].includes(".trim()"), "iter3: trim-corrected implementation");

  // Timing bounds: workerd's performance.now() advances in whole-millisecond
  // ticks (plan-review probe), so near-zero spans — the mock's LLM reply is
  // one resolved microtask, one transpileModule of a ~350-byte guest —
  // legitimately measure 0. llmMs/transpileMs assert presence/finite, not
  // positivity; typecheckMs/sandboxMs/totalMs include real work (tsc check +
  // QuickJS run) and keep their positive bounds. OBS-01 wants durations
  // recorded, not positive; 0 ms for a keyless mock call is honest.
  assert(
    run.iterations.every((it) => typeof it.llmMs === "number" && it.llmMs >= 0 && Number.isFinite(it.llmMs)),
    "slugify run: llmMs recorded (finite number) on every iteration",
  );
  assert(
    [it2, it3].every((it) => typeof it.transpileMs === "number" && it.transpileMs >= 0),
    "slugify run: transpileMs recorded on iterations 2-3",
  );
  const t = run.timings;
  assert(
    typeof t.llmMs === "number" && t.llmMs >= 0 && Number.isFinite(t.llmMs) &&
      t.typecheckMs > 0 &&
      typeof t.transpileMs === "number" && t.transpileMs >= 0 &&
      t.sandboxMs > 0 && t.totalMs > 0,
    "slugify run: aggregate timings recorded (typecheck/sandbox/total positive; llmMs/transpileMs finite >= 0)",
    t,
  );
  const parts = t.llmMs + t.typecheckMs + t.transpileMs + t.sandboxMs;
  assert(t.totalMs >= parts, "slugify run: totalMs >= sum of the per-step parts", { totalMs: t.totalMs, parts });

  // -- OBS-02: GET roundtrip + honest 404 --
  const replay = await get(portA, `/agent/run/${run.id}`);
  assert(
    replay.status === 200 && JSON.stringify(replay.json.data) === JSON.stringify(run),
    "GET /agent/run/:id roundtrips the stored record",
  );
  const unknownRun = await get(portA, "/agent/run/00000000-0000-4000-8000-000000000000");
  assert(unknownRun.status === 404 && unknownRun.json.error.code === "not_found", "GET unknown run id: 404 not_found");

  // -- Fibonacci scenario + generic fallback (LLM-02) --
  const fib = await post(portA, "/agent/run", JSON.stringify({ task: FIB_TASK, spec: FIB_SPEC }));
  assert(
    fib.status === 200 && fib.json.data.status === "success" && fib.json.data.iterations.length === 3,
    "fibonacci run: success in 3 iterations",
    fib.json.data.status,
  );
  const generic = await post(portA, "/agent/run", JSON.stringify({ task: "mystery assignment", spec: "do the thing" }));
  assert(
    generic.status === 200 && generic.json.data.status === "success",
    "generic task: converges to success",
    generic.json.data.status,
  );
  assert(
    generic.json.data.iterations[2].files["guest.ts"].includes("function fibonacci"),
    "generic task: falls back to the fibonacci implementation",
  );

  // -- Request-level cap: max_iterations with a single gate-failed iteration --
  const capped = await post(portA, "/agent/run", JSON.stringify({ task: SLUG_TASK, spec: SLUG_SPEC, maxIterations: 1 }));
  assert(
    capped.status === 200 && capped.json.data.status === "max_iterations",
    "maxIterations=1 request: terminal max_iterations",
    capped.json.data.status,
  );
  assert(
    capped.json.data.iterations.length === 1 &&
      capped.json.data.iterations[0].typecheck.ok === false &&
      capped.json.data.iterations[0].sandbox === null,
    "maxIterations=1 request: single typecheck-failed iteration, sandbox skipped",
  );

  // -- Validation (CORE-01 request shape) --
  const noSpec = await post(portA, "/agent/run", JSON.stringify({ task: "x" }));
  assert(noSpec.status === 400 && noSpec.json.error.code === "bad_request", "validation: missing spec 400 bad_request");
  const emptyTask = await post(portA, "/agent/run", JSON.stringify({ task: "", spec: "y" }));
  assert(emptyTask.status === 400 && emptyTask.json.error.code === "bad_request", "validation: empty task 400 bad_request");
  const agentMalformed = await post(portA, "/agent/run", "{nope");
  assert(agentMalformed.status === 400 && agentMalformed.json.error.code === "bad_request", "validation: malformed JSON 400 bad_request");
  const badCap = await post(portA, "/agent/run", JSON.stringify({ task: "x", spec: "y", maxIterations: 0 }));
  assert(badCap.status === 400 && badCap.json.error.code === "bad_request", "validation: maxIterations 0 rejected with 400 bad_request");

  // -- Phase 1 route regressions --
  const route404 = await get(portA, "/definitely-not-a-route");
  assert(route404.status === 404 && route404.json.error.code === "not_found", "route regression: 404 unknown route");
  const wrongMethod = await get(portA, "/agent/run");
  assert(
    wrongMethod.status === 405 && wrongMethod.headers.get("allow") === "POST",
    "route regression: 405 GET /agent/run with allow: POST",
  );

  // -- TYPE-03 stats sanity --
  const stats = await post(portA, "/typecheck", JSON.stringify({ files: { "guest.ts": GREEN_TS }, runs: 3 }));
  assert(
    stats.status === 200 && stats.json.data.runs === 3 && stats.json.data.results.length === 3,
    "typecheck stats: runs=3 -> 3 results",
  );
  assert(
    stats.json.data.results.every((r) => r.ok === true && r.ms > 0),
    "typecheck stats: every result ok with positive ms",
  );
  assert(stats.json.data.coldMs === stats.json.data.results[0].ms, "typecheck stats: coldMs is results[0].ms");
  const statsMax = Math.max(...stats.json.data.results.map((r) => r.ms));
  assert(
    stats.json.data.minMs <= stats.json.data.medianMs && stats.json.data.medianMs <= statsMax,
    "typecheck stats: min <= median <= max",
  );
  const redStats = await post(portA, "/typecheck", JSON.stringify({ files: { "guest.ts": RED_TS }, runs: 1 }));
  assert(redStats.json.data.results[0].ok === false, "typecheck stats: red snippet flags results[0].ok=false");
  const badRuns = await post(portA, "/typecheck", JSON.stringify({ files: { "guest.ts": GREEN_TS }, runs: 0 }));
  assert(badRuns.status === 400 && badRuns.json.error.code === "bad_request", "typecheck stats: runs=0 rejected with 400");
} finally {
  await killWrangler(wranglerA, portA);
}

// ---- Boot B: capped mock (MOCK_FAIL_AFTER=1, LLM_PROVIDER unset -> mock) ----
// G6 fires only for the mock provider (llm.id === MOCK_PROVIDER_ID), so this
// boot deliberately keeps LLM_PROVIDER unset: MOCK_FAIL_AFTER caps the
// effective maxIterations to 1 while the mock is still replaying its
// broken-first iteration -> terminal max_iterations after exactly 1 iteration.
let portB;
let wranglerB;
let logB = "";
try {
  portB = await freePort();
  wranglerB = spawnWrangler(portB, {
    MOCK_FAIL_AFTER: "1",
  });
  wranglerB.stdout.on("data", (d) => {
    logB += d;
  });
  wranglerB.stderr.on("data", (d) => {
    logB += d;
  });

  await waitHealthy(portB, () => logB);

  const capped = await post(portB, "/agent/run", JSON.stringify({ task: SLUG_TASK, spec: SLUG_SPEC }));
  assert(
    capped.status === 200 && capped.json.data.status === "max_iterations",
    "boot B: MOCK_FAIL_AFTER=1 caps the mock run to max_iterations",
    capped.json.data.status,
  );
  assert(capped.json.data.iterations.length === 1, "boot B: capped run has exactly 1 iteration", capped.json.data.iterations.length);
  assert(
    capped.json.data.iterations[0].typecheck.ok === false && capped.json.data.iterations[0].sandbox === null,
    "boot B: capped iteration failed typecheck with sandbox skipped",
  );
} finally {
  await killWrangler(wranglerB, portB);
}

// ---- Boot C: black-holed openai provider (llm_error + identity flip) ----
// In-process black-hole TCP server: swallows bytes, NEVER responds — the
// research probe pattern. The loop's fetch hangs until AbortSignal.timeout
// fires (LLM_TIMEOUT_MS=1000), the provider retries once (~2s total), and
// the run terminates llm_error.
const blackhole = createServer((socket) => {
  socket.on("data", () => {});
});
const blackholePort = await new Promise((resolve, reject) => {
  blackhole.once("error", reject);
  blackhole.listen(0, "127.0.0.1", () => resolve(blackhole.address().port));
});

let portC;
let wranglerC;
let logC = "";
try {
  portC = await freePort();
  wranglerC = spawnWrangler(portC, {
    LLM_PROVIDER: "openai",
    LLM_BASE_URL: `http://127.0.0.1:${blackholePort}`,
    LLM_API_KEY: "x",
    LLM_MODEL: "test",
    LLM_TIMEOUT_MS: "1000",
  });
  wranglerC.stdout.on("data", (d) => {
    logC += d;
  });
  wranglerC.stderr.on("data", (d) => {
    logC += d;
  });

  const healthzC = await waitHealthy(portC, () => logC);
  const runtimeC = healthzC.json.data.runtime;
  assert(
    runtimeC.agent === "openai-compatible",
    "boot C: agent identity flips to openai-compatible",
    runtimeC.agent,
  );

  // LLM-03 + CORE-04: black-holed openai endpoint -> retry-once inside the
  // provider -> terminal llm_error, zero iterations, retrievable via GET.
  const llmErr = await post(portC, "/agent/run", JSON.stringify({ task: SLUG_TASK, spec: SLUG_SPEC }));
  assert(
    llmErr.status === 200 && llmErr.json.data.status === "llm_error",
    "boot C: black-holed openai run ends llm_error (fetch + AbortSignal.timeout under no_nodejs_compat)",
    llmErr.json.data.status,
  );
  assert(llmErr.json.data.iterations.length === 0, "boot C: llm_error run stores zero iterations");
  const llmReplay = await get(portC, `/agent/run/${llmErr.json.data.id}`);
  assert(
    llmReplay.status === 200 && llmReplay.json.data.status === "llm_error",
    "boot C: llm_error run is retrievable with its stored status",
  );
} finally {
  await killWrangler(wranglerC, portC);
  blackhole.close();
}

if (failures.length > 0) {
  console.error(`\n${failures.length} assertion(s) failed.`);
  process.exitCode = 1;
}
