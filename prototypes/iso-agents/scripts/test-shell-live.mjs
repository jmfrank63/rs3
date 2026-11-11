// Live in-workerd verification harness for the Phase 5 agent shell
// (fourth leg of `npm run test:live`).
//
// ONE `wrangler dev` boot (default env — shell ops need no overrides), the
// proven duplicated-harness pattern from scripts/test-agent-live.mjs: free
// port, detached process group, port-closed teardown backstop. Leg ORDER
// matters for the lazy-identity asserts: the /healthz "unloaded" preflight
// comes FIRST (before any typecheck/run op), the identity flips come LAST.
//
// Sessions are STATELESS per request (CONTEXT bookkeeping): every leg builds
// its own VFS from its own seed/write ops — nothing leaks between requests
// (asserted). Guest sources are inlined copies of the mock scenarios
// (established harness pattern — src/ is TS and is never imported here).
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

const SLUGIFY_ITER1 = `function slugify(input: string): string {
  const slug: number = input.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return slug;
}
test("lowercases and hyphenates", () => { expect(slugify("Hello World")).toEqual("hello-world"); });
test("trims and collapses separators", () => { expect(slugify("  A -- B  ")).toEqual("a-b"); });
`;
const SLUGIFY_FINAL = `function slugify(input: string): string {
  return input.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}
test("lowercases and hyphenates", () => { expect(slugify("Hello World")).toEqual("hello-world"); });
test("trims and collapses separators", () => { expect(slugify("  A -- B  ")).toEqual("a-b"); });
`;
const WHILE_TRUE = "while(true) {}\n";

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

async function get(path, { method = "GET", body, headers } = {}) {
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

function post(path, bodyText) {
  return get(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: bodyText,
  });
}

async function waitHealthy(getLog) {
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

function spawnWrangler() {
  return spawn("wrangler", ["dev", "--port", String(port)], {
    env: { ...process.env, CI: "true" },
    stdio: ["ignore", "pipe", "pipe"],
    detached: true, // own process group so teardown kills wrangler + workerd together
  });
}

async function killWrangler() {
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

let port;
let wrangler;
let log = "";
try {
  port = await freePort();
  wrangler = spawnWrangler();
  wrangler.stdout.on("data", (d) => {
    log += d;
  });
  wrangler.stderr.on("data", (d) => {
    log += d;
  });

  // -- preflight: lazy identities still unloaded (BEFORE any typecheck/run) --
  const boot = await waitHealthy(() => log);
  assert(boot.status === 200, "boot: healthz HTTP 200");
  const runtime0 = boot.json.data.runtime;
  assert(runtime0.typechecker === "unloaded", "preflight: typechecker unloaded before any op", runtime0.typechecker);
  assert(runtime0.sandbox === "unloaded", "preflight: sandbox unloaded before any op", runtime0.sandbox);

  // -- 405 machinery comes free from KNOWN_PATHS --
  const wrongMethod = await get("/agent/shell");
  assert(
    wrongMethod.status === 405 && wrongMethod.headers.get("allow") === "POST",
    "405 GET /agent/shell with allow: POST",
  );

  // -- request-shape violations: 400 bad_request --
  const expect400 = async (bodyText, label) => {
    const res = await post("/agent/shell", bodyText);
    assert(res.status === 400 && res.json?.error?.code === "bad_request", label, res.status);
  };
  await expect400("{}", "validation: missing commands -> 400 bad_request");
  await expect400(JSON.stringify({ commands: [] }), "validation: empty commands -> 400 bad_request");
  await expect400(
    JSON.stringify({ commands: Array.from({ length: 33 }, () => ({ op: "ls" })) }),
    "validation: 33 commands -> 400 bad_request",
  );
  await expect400(JSON.stringify({ commands: ["ls"] }), "validation: non-object command -> 400 bad_request");
  await expect400(
    JSON.stringify({ commands: [{ op: "write", args: { file: "a.ts" } }] }),
    "validation: write without content -> 400 bad_request",
  );
  await expect400(JSON.stringify({ commands: [{ op: "cat" }] }), "validation: cat without file -> 400 bad_request");
  await expect400("{nope", "validation: malformed JSON -> 400 bad_request");
  await expect400(
    JSON.stringify({ commands: [{ op: "ls" }], files: { "a.ts": 1 } }),
    "validation: seed files non-string value -> 400 bad_request",
  );
  await expect400(
    JSON.stringify({ commands: [{ op: "ls" }], files: { "big.ts": "x".repeat(1048577) } }),
    "validation: seed over the 1 MiB VFS cap -> 400 bad_request",
  );
  await expect400(
    JSON.stringify({ commands: [{ op: "ls" }], stopOnError: "yes" }),
    "validation: stopOnError non-boolean -> 400 bad_request",
  );

  const session = (body) => post("/agent/shell", JSON.stringify(body));

  // -- op-level failures ride as 200 ok:false entries --
  const unknown = await session({ commands: [{ op: "frobnicate" }, { op: "ls" }] });
  assert(unknown.status === 200 && unknown.json.ok === true, "op-level: unknown op is a 200 ok envelope");
  assert(
    unknown.json.data.results[0].ok === false &&
      unknown.json.data.results[0].result.code === "unknown_op",
    "op-level: unknown op entry is ok:false unknown_op",
    unknown.json.data.results[0],
  );
  assert(
    unknown.json.data.haltedAt === undefined && unknown.json.data.results.length === 2,
    "op-level: session continues after an unknown op (default stopOnError=false)",
  );
  const notFound = await session({
    files: { "seed.ts": "const seed = 1;\n" },
    commands: [
      { op: "cat", args: { file: "missing.ts" } },
      { op: "rm", args: { file: "missing.ts" } },
    ],
  });
  assert(
    notFound.json.data.results.every((r) => r.ok === false && r.result.code === "not_found"),
    "op-level: cat/rm on a missing file are not_found",
  );
  assert(
    JSON.stringify(notFound.json.data.results[0].result.available) === '["/seed.ts"]',
    "op-level: not_found lists the available names",
  );
  const emptyCheck = await session({ commands: [{ op: "typecheck" }] });
  assert(
    emptyCheck.json.data.results[0].ok === false &&
      emptyCheck.json.data.results[0].result.code === "empty",
    "op-level: typecheck on an empty VFS is empty",
  );
  const vfsFull = await session({
    files: { "big.ts": "x".repeat(1048576) },
    commands: [
      { op: "write", args: { file: "small.ts", content: "y" } },
      { op: "ls" },
    ],
  });
  assert(
    vfsFull.json.data.results[0].ok === false &&
      vfsFull.json.data.results[0].result.code === "vfs_full",
    "op-level: write past the 1 MiB cap is vfs_full",
  );
  assert(
    vfsFull.json.data.results[1].result.files.length === 1,
    "op-level: the rejected write did not mutate the VFS",
  );

  // -- THE happy session (SHELL-01): write -> typecheck -> run 2/2 -> cat -> ls -> rm -> cat not-found -> ls --
  const happy = await session({
    commands: [
      { op: "write", args: { file: "guest.ts", content: SLUGIFY_FINAL } },
      { op: "typecheck" },
      { op: "run", args: { file: "guest.ts" } },
      { op: "cat", args: { file: "guest.ts" } },
      { op: "ls" },
      { op: "rm", args: { file: "guest.ts" } },
      { op: "cat", args: { file: "guest.ts" } },
      { op: "ls" },
    ],
  });
  assert(happy.status === 200 && happy.json.ok === true, "happy session: HTTP 200 ok envelope");
  const hr = happy.json.data.results;
  assert(hr.length === 8 && happy.json.data.haltedAt === undefined, "happy session: all 8 ops ran, no halt");
  assert(
    hr.every((r) => typeof r.ms === "number" && r.ms >= 0 && Number.isFinite(r.ms)),
    "happy session: every op ms recorded (finite >= 0)",
  );
  assert(
    hr[0].ok === true && hr[0].result.name === "/guest.ts" && hr[0].result.bytes === SLUGIFY_FINAL.length,
    "happy: write reports name + byte count",
  );
  assert(
    hr[1].ok === true && hr[1].result.ok === true && hr[1].result.diagnostics.length === 0,
    "happy: typecheck ok with 0 diagnostics",
  );
  assert(
    hr[2].ok === true && hr[2].result.sandbox.status === "ok",
    "happy: run executes with sandbox status ok",
  );
  assert(
    hr[2].result.sandbox.tests.length === 2 && hr[2].result.sandbox.tests.every((t) => t.passed === true),
    "happy: run registers 2/2 passing tests",
  );
  assert(hr[3].result.content === SLUGIFY_FINAL, "happy: cat roundtrips the exact content");
  assert(
    JSON.stringify(hr[4].result.files) ===
      JSON.stringify([{ name: "/guest.ts", bytes: SLUGIFY_FINAL.length }]),
    "happy: ls lists /guest.ts with its byte count",
  );
  assert(hr[5].ok === true && hr[5].result.removed === true, "happy: rm removes the file");
  assert(hr[6].ok === false && hr[6].result.code === "not_found", "happy: cat after rm is not_found");
  assert(hr[7].result.files.length === 0, "happy: ls is empty after rm");

  // -- per-request VFS isolation (CONTEXT bookkeeping decision) --
  const isolated = await session({ commands: [{ op: "ls" }] });
  assert(
    isolated.json.data.results[0].result.files.length === 0,
    "isolation: a new session starts with an empty VFS (nothing leaked)",
  );

  // -- error session: typecheck fails, run STILL executes (stopOnError=false default) --
  const broken = await session({
    commands: [
      { op: "write", args: { file: "guest.ts", content: SLUGIFY_ITER1 } },
      { op: "typecheck" },
      { op: "run", args: { file: "guest.ts" } },
    ],
  });
  const br = broken.json.data.results;
  assert(br.length === 3 && br[2].op === "run", "error session: run still executed (default stopOnError=false)");
  assert(br[1].ok === false && br[1].result.ok === false, "error session: the typecheck op fails");
  assert(
    br[1].result.diagnostics.length === 2,
    "error session: exactly 2 diagnostics",
    br[1].result.diagnostics?.length,
  );
  const d0 = br[1].result.diagnostics[0];
  assert(
    d0.code === 2322 && d0.line === 2 && d0.column === 9 && d0.file === "/guest.ts",
    "error session: TS2322 at 2:9 (1-based — same exactness the agent harness asserts)",
    d0,
  );
  assert(
    br[2].result.sandbox.status === "ok" &&
      br[2].result.sandbox.tests.filter((t) => t.passed === true).length === 2,
    "error session: the type-broken guest's logic still passes 2/2 — types are erased in the sandbox, proving the typecheck gate and the sandbox are independent layers",
  );

  // -- type-fixed / logic-broken (mock ITER2): typecheck PASSES, the run still
  // completes with tests 1/2 — the failed-test surface lives HERE (failing
  // tests are op-ok feedback material, not an execution failure) --
  const SLUGIFY_ITER2 = `function slugify(input: string): string {
  const slug: string = input.toLowerCase().replace(/[^a-z0-9]+/g, "-");
  return slug;
}
test("lowercases and hyphenates", () => { expect(slugify("Hello World")).toEqual("hello-world"); });
test("trims and collapses separators", () => { expect(slugify("  A -- B  ")).toEqual("a-b"); });
`;
  const logicBroken = await session({
    commands: [
      { op: "write", args: { file: "guest.ts", content: SLUGIFY_ITER2 } },
      { op: "typecheck" },
      { op: "run", args: { file: "guest.ts" } },
    ],
  });
  const lb = logicBroken.json.data.results;
  assert(
    lb[1].ok === true && lb[1].result.ok === true && lb[1].result.diagnostics.length === 0,
    "logic-broken session: typecheck passes (mock ITER2 is type-clean)",
  );
  assert(
    lb[2].ok === true &&
      lb[2].result.sandbox.status === "ok" &&
      lb[2].result.sandbox.tests.filter((t) => t.passed === true).length === 1,
    "logic-broken session: the run completes with tests 1/2 passing (a failing test is feedback, never dropped)",
  );

  // -- stopOnError halts BEFORE the run --
  const halted = await session({
    stopOnError: true,
    commands: [
      { op: "write", args: { file: "guest.ts", content: SLUGIFY_ITER1 } },
      { op: "typecheck" },
      { op: "run", args: { file: "guest.ts" } },
    ],
  });
  assert(
    halted.json.data.haltedAt === 1,
    "stopOnError: haltedAt is the failed typecheck's index",
    halted.json.data,
  );
  assert(
    halted.json.data.results.length === 2 && halted.json.data.results[1].op === "typecheck",
    "stopOnError: results stop at the failed op (the run never executed)",
  );

  // -- guardrail: while(true) run -> structured timeout, worker survives --
  const t0 = Date.now();
  const timedOut = await session({
    commands: [
      { op: "write", args: { file: "loop.ts", content: WHILE_TRUE } },
      { op: "run", args: { file: "loop.ts" } },
    ],
  });
  const tr = timedOut.json.data.results;
  assert(
    tr[1].ok === false && tr[1].result.sandbox.status === "timeout",
    "guardrail: while(true) run returns a structured timeout",
    tr[1].result.sandbox?.status,
  );
  assert(tr[1].ms <= 2250, "guardrail: the run op finished within the 2s sandbox deadline + 250ms", tr[1].ms);
  assert(Date.now() - t0 < 30000, "guardrail: the whole session returned promptly (wrangler dev never hangs)");
  const survived = await session({ commands: [{ op: "ls" }] });
  assert(survived.status === 200, "guardrail: the worker survives the timeout (subsequent ops answer)");

  // -- write upsert semantics --
  const upsert = await session({
    commands: [
      { op: "write", args: { file: "g.ts", content: "first\n" } },
      { op: "write", args: { file: "g.ts", content: "second!\n" } },
      { op: "ls" },
      { op: "cat", args: { file: "g.ts" } },
    ],
  });
  assert(upsert.json.data.results[2].result.files.length === 1, "upsert: ls still lists one entry");
  assert(
    upsert.json.data.results[3].result.content === "second!\n",
    "upsert: cat returns the latest content",
  );
  assert(
    upsert.json.data.results[1].result.totalBytes === 8,
    "upsert: totalBytes tracks the replacement",
    upsert.json.data.results[1].result,
  );

  // -- identity flips (LAST: the ops above did the lazy loading) --
  const flipped = await get("/healthz");
  const runtime = flipped.json.data.runtime;
  assert(
    runtime.typechecker === "typescript@6.0.3 (in-isolate)",
    "flips: typechecker identity flipped after the shell typecheck ops",
    runtime.typechecker,
  );
  assert(
    runtime.sandbox !== "unloaded",
    "flips: sandbox identity flipped after the shell run ops",
    runtime.sandbox,
  );
} finally {
  await killWrangler();
}

if (failures.length > 0) {
  console.error(`\n${failures.length} assertion(s) failed.`);
  process.exitCode = 1;
}
