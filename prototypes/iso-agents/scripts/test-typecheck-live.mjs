// Live in-workerd verification harness for the Phase 2 checker + Phase 4
// TYPE-03 stats (first leg of `npm run test:live`).
//
// Why this exists: the @cloudflare/vitest-plugin transport crashes workerd once the
// 8.8 MB typescript module enters the worker graph (WebSocket 1009 "Message is too
// large" — review issue 1, reproduced 4x), so NO vitest spec may pull the typecheck
// graph. Every typecheck/route/flip expectation runs here instead, against real
// workerd via `wrangler dev`.
//
// Phase 4 migration: POST /typecheck now returns N-run latency STATS
// {runs, results: [{run, ms, ok}], minMs, medianMs, meanMs, coldMs}; the old
// per-diagnostic exactness (TS2322 at 1-based column 7) RE-HOMED to
// scripts/test-agent-live.mjs — the agent loop's transcript carries real
// diagnostics (slugify iteration 1: TS2322 at /guest.ts:2:9). The old
// `501 POST /agent/run` probe became the wired route's 400-validation probes.
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

// Guest snippets — the Phase 2 semantics are unchanged, only the response
// shape around them moved to stats.
const GREEN = "export const answer: number = 42;\n";
const RED = 'const n: number = "not a number";\nconsole.log(n);\n';
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

  // -- Phase 1 healthz coverage (unchanged; `agent` key asserted by the agent harness) --
  assert(healthz.status === 200, "healthz: HTTP 200");
  assert(healthz.json.ok === true && healthz.json.data.status === "ok", "healthz: ok envelope with status ok");
  const runtime = healthz.json.data.runtime;
  assert(typeof runtime.workerd === "string" && runtime.workerd.length > 0, "healthz: runtime.workerd non-empty string");
  assert(typeof runtime.userAgent === "string" && runtime.userAgent.length > 0, "healthz: runtime.userAgent non-empty string");
  assert(runtime.compatibility_date === "2026-09-01", "healthz: compatibility_date is 2026-09-01");
  assert(runtime.typechecker === "unloaded", "healthz: typechecker is unloaded before any check", runtime.typechecker);
  assert(runtime.sandbox === "unloaded", "healthz: sandbox is unloaded");
  assert(typeof runtime.instanceId === "string" && runtime.instanceId.length === 36, "healthz: instanceId is a 36-char uuid");
  assert(typeof runtime.uptime_s === "number", "healthz: uptime_s is a number");

  // -- Phase 1 route coverage --
  const nf = await get("/nope");
  assert(nf.status === 404 && nf.json.error.code === "not_found", "404 unknown route returns not_found");
  const nfr = await get("/agent/run/00000000-0000-4000-8000-000000000000");
  assert(nfr.status === 404 && nfr.json.error.code === "not_found", "404 unknown run id returns not_found");
  const del = await get("/healthz", { method: "DELETE" });
  assert(
    del.status === 405 && del.json.error.code === "method_not_allowed" && del.headers.get("allow") === "GET",
    "405 DELETE /healthz returns allow: GET",
  );

  // -- Wired agent route: validation probes ONLY (a VALID body would run the
  // whole loop; that coverage lives in scripts/test-agent-live.mjs) --
  const noSpec = await post("/agent/run", JSON.stringify({ task: "x" }));
  assert(noSpec.status === 400 && noSpec.json.error.code === "bad_request", "agent route: missing spec 400 bad_request");
  const agentMalformed = await post("/agent/run", "{nope");
  assert(agentMalformed.status === 400 && agentMalformed.json.error.code === "bad_request", "agent route: malformed JSON 400 bad_request");

  // -- Green guest as an N-run benchmark (default runs = 5) --
  const green = await post("/typecheck", JSON.stringify({ files: { "guest.ts": GREEN } }));
  assert(green.status === 200, "green: HTTP 200 envelope");
  assert(green.json.ok === true, "green: envelope ok is true");
  assert(green.json.data.runs === 5, "green: default runs is 5");
  assert(green.json.data.results.length === 5, "green: five results");
  assert(green.json.data.results.every((r) => r.ok === true), "green: every result ok");
  assert(green.json.data.results.every((r) => r.ms > 0 && Number.isFinite(r.ms)), "green: every ms positive and finite");
  assert(green.json.data.coldMs === green.json.data.results[0].ms, "green: coldMs is results[0].ms");
  const greenMax = Math.max(...green.json.data.results.map((r) => r.ms));
  assert(
    green.json.data.minMs <= green.json.data.medianMs && green.json.data.medianMs <= greenMax,
    "green: minMs <= medianMs <= maxMs",
  );
  assert(green.json.data.meanMs > 0, "green: meanMs positive");

  // -- Red guest: red/green flag on the stats shape (diagnostic exactness
  // re-homed to the agent harness: slugify iter1 TS2322 at /guest.ts:2:9) --
  const red = await post("/typecheck", JSON.stringify({ files: { "guest.ts": RED }, runs: 1 }));
  assert(red.status === 200, "red: HTTP 200 even though the snippet has errors");
  assert(red.json.ok === true, "red: envelope ok is true");
  assert(red.json.data.results.length === 1 && red.json.data.results[0].ok === false, "red: single run flags ok=false");
  assert(red.json.data.results[0].ms > 0, "red: results[0].ms positive");

  // -- Lib closure + guest globals still check clean (2 runs) --
  const lib = await post("/typecheck", JSON.stringify({ files: { "guest.ts": LIB_GUEST }, runs: 2 }));
  assert(lib.status === 200 && lib.json.ok === true, "lib-sanity: HTTP 200 ok envelope");
  assert(
    lib.json.data.results.length === 2 && lib.json.data.results.every((r) => r.ok === true),
    "lib-sanity: Map/Promise.all/console guest checks clean",
  );

  // -- Memoization timing fidelity (runs=1 so coldMs IS the single check) --
  const first = await post("/typecheck", JSON.stringify({ files: { "guest.ts": GREEN }, runs: 1 }));
  const second = await post("/typecheck", JSON.stringify({ files: { "guest.ts": GREEN }, runs: 1 }));
  assert(first.json.data.coldMs > 0 && second.json.data.coldMs > 0, "memoization: both coldMs positive");
  assert(first.json.data.coldMs < 2000 && second.json.data.coldMs < 2000, "memoization: both under 2000ms");
  assert(second.json.data.coldMs <= first.json.data.coldMs + 100, "memoization: second <= first + 100ms");

  // -- TYPE-03 runs validation --
  const runsZero = await post("/typecheck", JSON.stringify({ files: { "guest.ts": GREEN }, runs: 0 }));
  assert(runsZero.status === 400 && runsZero.json.error.code === "bad_request", "runs=0: 400 bad_request");
  const runsGarbage = await post("/typecheck", JSON.stringify({ files: { "guest.ts": GREEN }, runs: "x" }));
  assert(runsGarbage.status === 400 && runsGarbage.json.error.code === "bad_request", 'runs="x": 400 bad_request');

  // -- files validation (Phase 2 cases 8+9, unchanged) --
  const empty = await post("/typecheck", JSON.stringify({ files: {} }));
  assert(empty.status === 400 && empty.json.error.code === "bad_request", "empty files object: 400 bad_request");
  const malformed = await post("/typecheck", "{nope");
  assert(malformed.status === 400 && malformed.json.error.code === "bad_request", "malformed JSON body: 400 bad_request");

  // -- 405 GET /typecheck asserts the allow RESPONSE HEADER --
  const notAllowed = await get("/typecheck");
  assert(notAllowed.status === 405, "GET /typecheck: 405 status");
  assert(
    notAllowed.headers.get("allow") === "POST",
    "GET /typecheck: allow: POST response header (never the body)",
  );
  assert(notAllowed.json.error.code === "method_not_allowed", "GET /typecheck: method_not_allowed error code");

  // -- Identity flip (Phase 2 case 10) --
  const flip = await get("/healthz");
  assert(
    flip.status === 200 && flip.json.data.runtime.typechecker === "typescript@6.0.3 (in-isolate)",
    "identity flip: typescript@6.0.3 (in-isolate) after first check",
    flip.json.data.runtime.typechecker,
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
