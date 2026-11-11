import { runRegistry } from "./runs";
import { getTypechecker, getTypecheckerIdentity } from "./typecheck";
import { parseTSChecker } from "./typecheck/selection";
import { GuestFileError, WasmGuestTooComplexError, assertGuestWithinWasmLimits, assertValidGuestFile } from "./typecheck-wasm/guard";
import { transpileGuest } from "./typecheck/transpile";
import { getSandboxRuntime } from "./sandbox/runtime";
import { getSandboxIdentity } from "./sandbox/identity";
import {
  createLlmProvider,
  DEFAULT_LLM_TIMEOUT_MS,
  getAgentIdentity,
  type LlmProvider,
} from "./agent/provider";
import { MOCK_PROVIDER_ID } from "./agent/mock";
import { runAgent, AgentLoopError } from "./agent/loop";
import { runShellSession, ShellBadRequestError, type ShellSessionResult } from "./shell/shell";

export interface Env {
  // WASM-03: TSCHECKER selects the Typechecker — "ts-js" (default) or
  // "tsc-rs-wasm" (the factory lives in src/typecheck/index.ts). Surfaced by
  // the /healthz identity.
  TSCHECKER?: string;
  // FORK-02: TSCHECKER_WASM_RECYCLE_AFTER overrides the held wasm
  // instance's recycle threshold (default 50 = RECYCLE_AFTER_RUNS in
  // src/typecheck-wasm/shim.ts). Parsed by src/typecheck/selection.ts; the
  // live harness boots with 2, the bench's fresh leg with 1.
  TSCHECKER_WASM_RECYCLE_AFTER?: string;
  LLM_PROVIDER?: string;
  LLM_BASE_URL?: string;
  LLM_API_KEY?: string;
  LLM_MODEL?: string;
  LLM_TIMEOUT_MS?: string;
  AGENT_MAX_ITERATIONS?: string;
  AGENT_RUN_DEADLINE_MS?: string;
  MOCK_FAIL_AFTER?: string;
  SANDBOX_TIMEOUT_MS?: string;
  SANDBOX_MEMORY_LIMIT_MB?: string;
  SANDBOX_STACK_KB?: string;
}

const COMPATIBILITY_DATE = "2026-09-01";
const DEFAULT_MAX_ITERATIONS = 5;
const MAX_ITERATIONS_CAP = 10;
const DEFAULT_RUN_DEADLINE_MS = 25000;
const DEFAULT_BENCH_RUNS = 5;
const MAX_BENCH_RUNS = 100;

// Per isolate; surfaces restarts. workerd freezes Date.now() in global scope
// (module init reads ~0), so boot time is captured lazily on first request
// and memoized for the isolate's lifetime.
let bootMs: number | undefined;
function bootMsOnce(): number {
  return (bootMs ??= Date.now());
}
// Per isolate boot; surfaces restarts. workerd disallows random-value
// generation in global scope, so the id is generated lazily on first request
// and memoized for the isolate's lifetime.
let instanceId: string | undefined;
function instanceIdOnce(): string {
  return (instanceId ??= crypto.randomUUID());
}

type ErrorCode =
  | "bad_request"
  | "not_found"
  | "method_not_allowed"
  | "not_implemented"
  | "internal";

type Envelope =
  | { ok: true; data: unknown }
  | { ok: false; error: { code: ErrorCode; message: string; detail?: unknown } };

function json(body: Envelope, status: number, headers?: Record<string, string>): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8", ...headers },
  });
}

function fail(
  status: number,
  code: ErrorCode,
  message: string,
  detail?: unknown,
  headers?: Record<string, string>,
): Response {
  const error = detail !== undefined ? { code, message, detail } : { code, message };
  return json({ ok: false, error }, status, headers);
}

class HttpError extends Error {
  constructor(
    readonly status: number,
    readonly code: ErrorCode,
    message: string,
    readonly detail?: unknown,
  ) {
    super(message);
  }
}

// Pre-wired for POST routes (400 bad_request on malformed JSON).
async function parseJsonBody(request: Request): Promise<unknown> {
  try {
    return await request.json();
  } catch {
    throw new HttpError(400, "bad_request", "request body is not valid JSON");
  }
}

// v5 H2: under TSCHECKER=tsc-rs-wasm the checker owns /app/tsconfig.json and
// /app/guest-globals.d.ts; a guest file with either name used to overwrite /
// be shadowed by the compiler-owned seed. Validated AFTER canonicalization,
// BEFORE the (cold ~500 ms) wasm check runs, as 400 bad_request. ts-js mode
// is untouched: there the tsconfig name simply checks as a red TS parse.
function assertWasmGuestFiles(files: Map<string, string>, env: Env): void {
  if (parseTSChecker(env) !== "tsc-rs-wasm") {
    return;
  }
  for (const [name, content] of files) {
    try {
      assertValidGuestFile(name);
      assertGuestWithinWasmLimits(name, content);
    } catch (err) {
      if (err instanceof GuestFileError || err instanceof WasmGuestTooComplexError) {
        throw new HttpError(400, "bad_request", err.message);
      }
      throw err;
    }
  }
}

// Phase 1 sandbox limits use this exact shape (src/sandbox/limits.ts).
function parseOptionalPositiveInt(raw: string | undefined, fallback: number): number {
  if (raw === undefined) return fallback;
  const n = Number(raw);
  return Number.isInteger(n) && n > 0 ? n : fallback;
}

interface TypecheckBody {
  files?: Record<string, unknown>;
  runs?: unknown;
}

// TYPE-02 + TYPE-03: N-run latency stats — the tsc-rs benchmark hook. Same
// checker instance N times per request: results[0].ms is that request's cold
// run, the rest are warm. `results[].ok` is a research-G2 ADDITION to the
// CONTEXT stats shape so harnesses keep a red/green assert; per-diagnostic
// exactness re-homed to scripts/test-agent-live.mjs (agent transcript).
async function handleTypecheck(request: Request, env: Env): Promise<Response> {
  const body = (await parseJsonBody(request)) as TypecheckBody | null;
  if (
    body === null ||
    typeof body !== "object" ||
    body.files === undefined ||
    body.files === null ||
    typeof body.files !== "object"
  ) {
    throw new HttpError(400, "bad_request", "body must be JSON {files: {name: content}, runs?: number}");
  }
  const entries = Object.entries(body.files);
  if (entries.length === 0) {
    throw new HttpError(400, "bad_request", "files must contain at least one entry");
  }
  const bad = entries.find(([, content]) => typeof content !== "string");
  if (bad !== undefined) {
    throw new HttpError(400, "bad_request", `files.${bad[0]} must be a string`);
  }
  let runs = DEFAULT_BENCH_RUNS;
  if (body.runs !== undefined) {
    if (
      typeof body.runs !== "number" ||
      !Number.isInteger(body.runs) ||
      body.runs < 1 ||
      body.runs > MAX_BENCH_RUNS
    ) {
      throw new HttpError(400, "bad_request", `runs must be an integer between 1 and ${MAX_BENCH_RUNS}`);
    }
    runs = body.runs;
  }
  const files = new Map<string, string>(
    entries.map(([name, content]): [string, string] => [
      name.startsWith("/") ? name : `/${name}`,
      content as string,
    ]),
  );
  assertWasmGuestFiles(files, env);
  const checker = getTypechecker(env);
  const results: Array<{ run: number; ms: number; ok: boolean }> = [];
  for (let i = 1; i <= runs; i++) {
    const r = await checker.check(files);
    results.push({ run: i, ms: r.ms, ok: r.ok });
  }
  // FORK-03 observability: the wasm checker's held-instance stats ride the
  // response — same instanceGen across runs = reuse; a bump = recycle — so
  // harnesses prove instance reuse + recycling over plain HTTP. Other
  // checkers omit the field entirely.
  const held = checker.heldStats?.();
  // Stats on a sorted COPY; `results` stays in run order (run r == results[r-1]).
  const sorted = results.map((r) => r.ms).sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  const medianMs = sorted.length % 2 === 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
  const meanMs = sorted.reduce((sum, ms) => sum + ms, 0) / sorted.length;
  return json(
    {
      ok: true,
      data: {
        runs,
        results,
        minMs: sorted[0],
        medianMs,
        meanMs,
        coldMs: results[0].ms,
        ...(held === undefined ? {} : { held }),
      },
    },
    200,
  );
}

interface AgentRunBody {
  task?: unknown;
  spec?: unknown;
  maxIterations?: unknown;
}

// CORE-01/02/04/05 + OBS-01: the SYNCHRONOUS agent loop (CONTEXT — the mock
// finishes in seconds; async/job polling is out of scope). The provider is
// built PER REQUEST (mock call-counter state is per-run), the record is
// created BEFORE the loop and finalized AFTER it, and the response IS the
// stored record (POST and GET return byte-identical data). v5 H8: an
// unexpected failure anywhere in construction or the loop finalizes the run
// with a terminal `internal_error` (partial transcript preserved via
// AgentLoopError) BEFORE the Phase 1 500 envelope is returned — no stored
// record can stay "running" while its request already failed.
async function handleAgentRun(request: Request, env: Env): Promise<Response> {
  const body = (await parseJsonBody(request)) as AgentRunBody | null;
  if (
    body === null ||
    typeof body !== "object" ||
    typeof body.task !== "string" ||
    body.task.trim() === "" ||
    typeof body.spec !== "string" ||
    body.spec.trim() === ""
  ) {
    throw new HttpError(
      400,
      "bad_request",
      "body must be JSON {task: string, spec: string, maxIterations?: number 1-10}",
    );
  }
  if (
    body.maxIterations !== undefined &&
    (typeof body.maxIterations !== "number" ||
      !Number.isInteger(body.maxIterations) ||
      body.maxIterations < 1 ||
      body.maxIterations > MAX_ITERATIONS_CAP)
  ) {
    throw new HttpError(400, "bad_request", `maxIterations must be an integer between 1 and ${MAX_ITERATIONS_CAP}`);
  }
  const requested =
    body.maxIterations !== undefined
      ? body.maxIterations
      : parseOptionalPositiveInt(env.AGENT_MAX_ITERATIONS, DEFAULT_MAX_ITERATIONS);
  let maxIterations = Math.min(Math.max(requested, 1), MAX_ITERATIONS_CAP);
  const runDeadlineMs = parseOptionalPositiveInt(env.AGENT_RUN_DEADLINE_MS, DEFAULT_RUN_DEADLINE_MS);
  const llmTimeoutMs = parseOptionalPositiveInt(env.LLM_TIMEOUT_MS, DEFAULT_LLM_TIMEOUT_MS);

  const run = runRegistry.create(body.task);
  try {
    let llm: LlmProvider;
    try {
      llm = createLlmProvider(env, llmTimeoutMs);
    } catch {
      // Misconfigured openai env ("requires LLM_BASE_URL and LLM_MODEL") —
      // honest terminal state on the record, not a 500 (research decision).
      runRegistry.finalize(run, "llm_error", [], null);
      return json({ ok: true, data: run }, 200);
    }
    // G6 semantics: MOCK_FAIL_AFTER caps the loop's maxIterations for MOCK runs
    // only, so a capped mock run ends max_iterations while the mock is still
    // replaying its broken-first iterations.
    if (llm.id === MOCK_PROVIDER_ID) {
      maxIterations = Math.min(maxIterations, parseOptionalPositiveInt(env.MOCK_FAIL_AFTER, Number.MAX_SAFE_INTEGER));
    }
    const result = await runAgent({
      task: body.task,
      spec: body.spec,
      maxIterations,
      runDeadlineMs,
      llm,
      typechecker: getTypechecker(env),
      sandbox: getSandboxRuntime(env),
      transpile: transpileGuest,
    });
    // finalize mutates the stored record IN PLACE — `run` IS the stored record.
    runRegistry.finalize(run, result.status, result.iterations, result.timings);
    return json({ ok: true, data: run }, 200);
  } catch (err) {
    // v5 H8: checker/transpiler/sandbox/module exceptions used to bypass
    // finalization entirely (the record stayed `running` while the outer
    // fetch handler answered 500). Finalize with a terminal internal_error
    // and the partial transcript, then re-raise through the same 500
    // envelope the Phase 1 route contract promises.
    const iterations = err instanceof AgentLoopError ? err.iterations : run.iterations;
    console.error(`[worker] POST /agent/run failed (run ${run.id}):`, err);
    runRegistry.finalize(run, "internal_error", iterations, null);
    const message = err instanceof Error ? err.message : String(err);
    return fail(500, "internal", `agent run failed: ${message}`, { runId: run.id });
  }
}

interface SandboxExecuteBody {
  code?: unknown;
}

// SAND-01..04 debug surface (prototype affordance): executes posted JS in the
// QuickJS-WASM sandbox and returns the structured ExecuteResult. 200 even
// when data.status is error/timeout/crash — the execution itself succeeded.
async function handleSandboxExecute(request: Request, env: Env): Promise<Response> {
  const body = (await parseJsonBody(request)) as SandboxExecuteBody | null;
  if (body === null || typeof body !== "object" || typeof body.code !== "string") {
    throw new HttpError(400, "bad_request", "body must be JSON {code: string}");
  }
  const result = await getSandboxRuntime(env).execute(body.code);
  return json({ ok: true, data: result }, 200);
}

interface ShellBody {
  commands?: unknown;
  files?: unknown;
  stopOnError?: unknown;
}

// SHELL-01/02: stateless tool-protocol session (CONTEXT — no shell process,
// no shared state; per-request VFS seeded only by `files`). Request-shape
// violations — including the ShellBadRequestError the pure session module
// throws — are 400 bad_request; per-op failures are 200 ok:false entries.
// Deps are the SAME per-request primitives /agent/run uses
// (getTypechecker(env) singleton, getSandboxRuntime(env), transpileGuest), so
// the sandbox/typecheck guardrails are inherited by construction and the
// agent loop stays untouched (CONTEXT decision).
async function handleShell(request: Request, env: Env): Promise<Response> {
  const body = (await parseJsonBody(request)) as ShellBody | null;
  if (body === null || typeof body !== "object") {
    throw new HttpError(
      400,
      "bad_request",
      "body must be JSON {commands: [{op, args?}...], files?, stopOnError?}",
    );
  }
  let session: ShellSessionResult;
  try {
    session = await runShellSession(
      { commands: body.commands, files: body.files, stopOnError: body.stopOnError },
      { typechecker: getTypechecker(env), sandbox: getSandboxRuntime(env), transpile: transpileGuest },
    );
  } catch (err) {
    if (err instanceof ShellBadRequestError) {
      throw new HttpError(400, "bad_request", err.message);
    }
    // v5 H2: the wasm checker's seed guard rejects compiler-owned guest
    // paths (the typecheck op checks the session VFS) — same 400 class.
    if (err instanceof GuestFileError) {
      throw new HttpError(400, "bad_request", err.message);
    }
    throw err;
  }
  return json({ ok: true, data: session }, 200);
}

// Lazy-init seam: Phases 2-4 fill typechecker/sandbox/agent identities;
// /healthz stays fast. `agent` is appended LAST so the Phase 1 payload key
// order stays stable (CONTEXT).
export function getRuntimeInfo(env: Env) {
  return {
    // Best-effort workerd version: workerd has no in-band version API, so
    // navigator.userAgent is the only in-band signal (tests assert non-empty).
    workerd: navigator.userAgent,
    compatibility_date: COMPATIBILITY_DATE,
    typechecker: getTypecheckerIdentity(env), // flips after the SELECTED checker's first use (lazy singleton)
    sandbox: getSandboxIdentity(), // flips after first sandbox execution (lazy module load)
    instanceId: instanceIdOnce(),
    uptime_s: Math.floor((Date.now() - bootMsOnce()) / 1000),
    userAgent: navigator.userAgent, // raw signal behind the `workerd` best-effort above
    agent: getAgentIdentity(env), // LLM_PROVIDER-selected identity, appended last
  };
}

const KNOWN_PATHS: Record<string, string> = {
  "/healthz": "GET",
  "/agent/run": "POST",
  "/agent/shell": "POST",
  "/typecheck": "POST",
  "/sandbox/execute": "POST",
};

export default {
  async fetch(request: Request, env: Env, _ctx: ExecutionContext): Promise<Response> {
    const started = performance.now();
    const { method, pathname } = { method: request.method, pathname: new URL(request.url).pathname };
    const label = `${method} ${pathname}`;
    const respond = (res: Response): Response => {
      console.log(`[worker] ${label} -> ${res.status} (${(performance.now() - started).toFixed(1)}ms)`);
      return res;
    };
    try {
      switch (`${method} ${pathname}`) {
        case "GET /healthz":
          return respond(json({ ok: true, data: { status: "ok", runtime: getRuntimeInfo(env) } }, 200));
        case "POST /agent/run":
          return respond(await handleAgentRun(request, env));
        case "POST /typecheck":
          return respond(await handleTypecheck(request, env));
        case "POST /agent/shell":
          return respond(await handleShell(request, env));
        case "POST /sandbox/execute":
          return respond(await handleSandboxExecute(request, env));
        default: {
          if (method === "GET" && pathname.startsWith("/agent/run/")) {
            const id = pathname.slice("/agent/run/".length);
            const run = runRegistry.get(id);
            if (run === undefined) {
              return respond(
                fail(404, "not_found", `run ${id} not found`, {
                  id,
                  hint: "in-memory store; isolate restart clears it",
                }),
              );
            }
            return respond(json({ ok: true, data: run }, 200));
          }
          const allowedMethod = KNOWN_PATHS[pathname];
          if (allowedMethod !== undefined) {
            return respond(
              fail(405, "method_not_allowed", `${label} not allowed; use ${allowedMethod} ${pathname}`, undefined, {
                allow: allowedMethod,
              }),
            );
          }
          return respond(fail(404, "not_found", `no route for ${label}`));
        }
      }
    } catch (err) {
      const status = err instanceof HttpError ? err.status : 500;
      const code = err instanceof HttpError ? err.code : "internal";
      const message = err instanceof Error ? err.message : String(err);
      const detail = err instanceof HttpError ? err.detail : { instanceId: instanceIdOnce() };
      console.error(`[worker] ${label} failed:`, err);
      return respond(fail(status, code, message, detail));
    }
  },
} satisfies ExportedHandler<Env>;
