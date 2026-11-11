// The agent loop (CORE-02..05): a pure orchestrator — LLM, typechecker,
// sandbox and transpile are injected. NO runtime import may reach
// src/typecheck/ or src/sandbox/ (the 8.8 MB typescript module crashes the
// vitest worker transport — Phase 2 lesson): seams enter as `import type`,
// transpilation as an injected function. Per-iteration pipeline (CORE-02):
// deadline check -> LLM -> contract extraction (one corrective retry) ->
// typecheck gate (CORE-03: failure skips the sandbox) -> single-file gate ->
// transpile -> sandbox -> feedback or success. All timings via
// performance.now() (OBS-01); statuses are the CONTEXT terminal set (CORE-04).
import type { ExecuteResult, SandboxRuntime } from "../sandbox/runtime";
import type { TypecheckDiagnostic, TypecheckResult, Typechecker } from "../typecheck/index";
import type { TranspileResult } from "../typecheck/transpile";
import type { LlmContract } from "./extractor";
import type { LlmMessage, LlmProvider } from "./provider";
import { extractContract, MalformedContractError, ScriptViolationError } from "./extractor";

export type AgentStatus = "success" | "max_iterations" | "run_deadline" | "llm_error" | "llm_malformed";

export interface AgentIteration {
  n: number;
  llmMs: number;
  explanation: string;
  files: Record<string, string>;
  typecheck: TypecheckResult | null;
  transpileMs: number | null;
  sandbox: ExecuteResult | null;
  feedback: string | null;
}

export interface AgentTimings {
  llmMs: number;
  typecheckMs: number;
  transpileMs: number;
  sandboxMs: number;
  totalMs: number;
}

export interface AgentRunResult {
  status: AgentStatus;
  iterations: AgentIteration[];
  timings: AgentTimings;
}

// v5 H8: an unexpected seam failure (checker crash, transpiler bug, sandbox
// module failure) must not leave the run record "running" forever. The loop
// wraps EVERYTHING it iterates; on a throw it attaches the iterations
// completed so far as the partial transcript so the HTTP layer can finalize
// honestly before answering 500.
export class AgentLoopError extends Error {
  constructor(
    readonly iterations: AgentIteration[],
    cause: unknown,
  ) {
    super(cause instanceof Error ? cause.message : String(cause), { cause });
  }
}

export interface AgentLoopDeps {
  task: string;
  spec: string;
  maxIterations: number;
  runDeadlineMs: number;
  llm: LlmProvider;
  typechecker: Typechecker;
  sandbox: SandboxRuntime;
  transpile: (source: string) => TranspileResult;
}

// CONTEXT prompt strategy: senior-TS role + the hard rules, including the
// sandbox contract (script-style, sync-only, injected test/expect/assert).
export const SYSTEM_PROMPT = [
  "You are a senior TypeScript engineer implementing a small pure function.",
  "Reply with ONLY a JSON object, no prose around it, exactly:",
  '{"explanation": "<what you did>", "files": {"<filename>": "<full file content>"}}',
  "Hard rules:",
  "- The code must type-check cleanly under strict ES2022.",
  "- Every test must pass.",
  "- Return exactly ONE script-style file: NO import or export statements anywhere.",
  "- No async, no await, no Promise (the runtime is synchronous).",
  "- The runtime injects test(name, fn), expect(actual).toEqual/toBeTrue/toBeCloseTo, assert(cond, msg?) and console.",
  "- Include test(\"name\", () => { expect(...) }) calls that cover the spec.",
  "- Return the FULL content of every file every time, never a diff.",
].join("\n");

const CORRECTIVE_PROMPT = "respond with ONLY valid JSON";
const FEEDBACK_WINDOW = 3;

// CONTEXT feedback format for type errors: `file:line:col TSxxxx: message`.
function formatDiagnostics(diagnostics: TypecheckDiagnostic[]): string {
  return diagnostics
    .filter((d) => d.category === "Error")
    .map((d) => `${d.file}:${d.line}:${d.column} TS${d.code}: ${d.message}`)
    .join("\n");
}

// CONTEXT feedback for sandbox failures: failed test names + assertion
// errors + captured logs + "fix and return the FULL file(s) again".
function formatSandboxFeedback(exec: ExecuteResult): string {
  const lines: string[] = [];
  for (const failed of exec.tests.filter((t) => !t.passed)) {
    lines.push(`FAILED test "${failed.name}"${failed.error === undefined ? "" : `: ${failed.error}`}`);
  }
  for (const log of exec.logs) {
    lines.push(`guest ${log.level}: ${log.args.map((a) => JSON.stringify(a) ?? String(a)).join(" ")}`);
  }
  if (exec.status !== "ok" && exec.error !== undefined) {
    lines.push(`sandbox ${exec.status}: ${exec.error}`);
  }
  lines.push("fix and return the FULL file(s) again");
  return lines.join("\n");
}

function rootFiles(files: Record<string, string>): Map<string, string> {
  return new Map(
    Object.entries(files).map(([name, content]): [string, string] => [
      name.startsWith("/") ? name : `/${name}`,
      content,
    ]),
  );
}

async function runLoop(deps: AgentLoopDeps, iterations: AgentIteration[]): Promise<AgentRunResult> {
  const t0 = performance.now();
  const deadline = t0 + deps.runDeadlineMs;
  // G10 + v5 H4: the deadline is ONE wall-clock budget captured at run
  // start, enforced after EVERY awaited stage (LLM call, corrective retry,
  // typecheck, sandbox) and before returning success — never only before
  // the next LLM call. Otherwise a stage that overruns the budget (slow
  // provider, cold wasm check) could still be followed by check+execute
  // and a `success` verdict issued after the deadline.
  const expired = (): boolean => performance.now() >= deadline;
  const totals = { llmMs: 0, typecheckMs: 0, transpileMs: 0, sandboxMs: 0 };
  const finish = (status: AgentStatus): AgentRunResult => ({
    status,
    iterations,
    timings: { ...totals, totalMs: performance.now() - t0 },
  });
  const baseMessages: LlmMessage[] = [
    { role: "system", content: SYSTEM_PROMPT },
    { role: "user", content: `${deps.task}\n\nSPEC:\n${deps.spec}\n\nReturn ONLY the JSON contract.` },
  ];
  const feedbackHistory: LlmMessage[] = [];

  for (let n = 1; n <= deps.maxIterations; n++) {
    if (expired()) {
      return finish("run_deadline");
    }
    // CONTEXT windowing: system + task/spec first user message + last 3
    // feedback messages. No assistant-role echoes.
    const messages: LlmMessage[] = [...baseMessages, ...feedbackHistory.slice(-FEEDBACK_WINDOW)];
    let iterLlmMs = 0;

    const llmStart = performance.now();
    let text: string;
    try {
      text = (await deps.llm.complete(messages, { deadlineMs: deadline })).text;
    } catch {
      iterLlmMs = performance.now() - llmStart;
      totals.llmMs += iterLlmMs;
      // The provider failure may BE the deadline (clamped attempt timed
      // out / budget refused) — the honest terminal status then is ours.
      return finish(expired() ? "run_deadline" : "llm_error");
    }
    iterLlmMs = performance.now() - llmStart;
    totals.llmMs += iterLlmMs;
    if (expired()) {
      return finish("run_deadline");
    }

    let contract: LlmContract | undefined;
    let violationFeedback: string | undefined;
    let malformedMessage: string | undefined;
    let retryLlmFailed = false;
    try {
      contract = extractContract(text);
    } catch (err) {
      if (err instanceof ScriptViolationError) {
        // G3: script violations heal via feedback, never terminally.
        violationFeedback = `${err.message}; fix and return the FULL file(s) again`;
      } else if (err instanceof MalformedContractError && performance.now() < deadline) {
        // G9: exactly one corrective retry per malformed iteration — its
        // LLM call inherits the same remaining budget (H4).
        const retryStart = performance.now();
        try {
          const corrective: LlmMessage = { role: "user", content: CORRECTIVE_PROMPT };
          text = (await deps.llm.complete([...messages, corrective], { deadlineMs: deadline })).text;
          contract = extractContract(text);
        } catch (retryErr) {
          if (retryErr instanceof ScriptViolationError) {
            violationFeedback = `${retryErr.message}; fix and return the FULL file(s) again`;
          } else if (retryErr instanceof MalformedContractError) {
            malformedMessage = retryErr.message;
          } else {
            retryLlmFailed = true;
          }
        } finally {
          const retryMs = performance.now() - retryStart;
          iterLlmMs += retryMs;
          totals.llmMs += retryMs;
        }
      } else if (err instanceof MalformedContractError) {
        malformedMessage = err.message;
      } else {
        malformedMessage = "contract extraction failed";
      }
    }

    // H4: the corrective-retry LLM call is an awaited stage — enforce the
    // budget before any gate runs. (A script-violation healing iteration
    // falls through to its push+continue below, where the loop-top check
    // records the iteration AND terminates as run_deadline.)
    if (expired() && violationFeedback === undefined) {
      return finish("run_deadline");
    }

    if (contract === undefined && violationFeedback !== undefined) {
      iterations.push({
        n,
        llmMs: iterLlmMs,
        explanation: "",
        files: {},
        typecheck: null,
        transpileMs: null,
        sandbox: null,
        feedback: violationFeedback,
      });
      feedbackHistory.push({ role: "user", content: violationFeedback });
      continue;
    }
    if (contract === undefined) {
      if (retryLlmFailed) {
        const fb = "llm request failed during the corrective retry";
        iterations.push({
          n,
          llmMs: iterLlmMs,
          explanation: "",
          files: {},
          typecheck: null,
          transpileMs: null,
          sandbox: null,
          feedback: fb,
        });
        return finish("llm_error");
      }
      const fb = malformedMessage ?? "contract extraction failed";
      iterations.push({
        n,
        llmMs: iterLlmMs,
        explanation: "",
        files: {},
        typecheck: null,
        transpileMs: null,
        sandbox: null,
        feedback: fb,
      });
      return finish("llm_malformed");
    }

    // CORE-03 gate: a failing type-check SKIPS the sandbox entirely.
    const check = await deps.typechecker.check(rootFiles(contract.files));
    totals.typecheckMs += check.ms;
    if (expired()) {
      return finish("run_deadline");
    }
    if (!check.ok) {
      const fb = `type errors — fix and return the FULL file(s) again:\n${formatDiagnostics(check.diagnostics)}`;
      iterations.push({
        n,
        llmMs: iterLlmMs,
        explanation: contract.explanation,
        files: contract.files,
        typecheck: check,
        transpileMs: null,
        sandbox: null,
        feedback: fb,
      });
      feedbackHistory.push({ role: "user", content: fb });
      continue;
    }

    // Sandbox executes ONE script (REQUIREMENTS out-of-scope: multi-file
    // bundling) — enforce the system prompt's single-file rule with real
    // feedback instead of silently running an arbitrary first file.
    const names = Object.keys(contract.files);
    if (names.length !== 1) {
      const fb =
        "return exactly ONE file (the sandbox executes a single script); fix and return the FULL file(s) again";
      iterations.push({
        n,
        llmMs: iterLlmMs,
        explanation: contract.explanation,
        files: contract.files,
        typecheck: check,
        transpileMs: null,
        sandbox: null,
        feedback: fb,
      });
      feedbackHistory.push({ role: "user", content: fb });
      continue;
    }

    const tr = deps.transpile(contract.files[names[0]]);
    totals.transpileMs += tr.ms;
    const errorDiags = tr.diagnostics.filter((d) => d.category === "Error");
    if (errorDiags.length > 0) {
      // Near-impossible post-gate (transpileModule reports syntactic diags
      // only) — kept as a cheap tripwire with honest feedback.
      const fb = `transpile errors — fix and return the FULL file(s) again:\n${formatDiagnostics(errorDiags)}`;
      iterations.push({
        n,
        llmMs: iterLlmMs,
        explanation: contract.explanation,
        files: contract.files,
        typecheck: check,
        transpileMs: tr.ms,
        sandbox: null,
        feedback: fb,
      });
      feedbackHistory.push({ role: "user", content: fb });
      continue;
    }

    const exec = await deps.sandbox.execute(tr.js);
    totals.sandboxMs += exec.ms;
    // Success predicate: the guest evaluated AND registered tests AND every
    // test passed. A test-less guest is a failed iteration (guards an LLM
    // answer that "passes" by never asserting). H4: even a fully green
    // iteration NEVER returns success after the deadline — the completed
    // iteration stays in the transcript, but the terminal status is honest.
    if (exec.status === "ok" && exec.tests.length > 0 && exec.tests.every((t) => t.passed)) {
      iterations.push({
        n,
        llmMs: iterLlmMs,
        explanation: contract.explanation,
        files: contract.files,
        typecheck: check,
        transpileMs: tr.ms,
        sandbox: exec,
        feedback: null,
      });
      if (expired()) {
        return finish("run_deadline");
      }
      return finish("success");
    }

    // v5 H11: a guest that never ran cleanly (timeout/crash/gate rejection)
    // used to be reported as ONLY "no test() calls found" when it had
    // registered zero tests — hiding the real failure from the model. The
    // execution status + error lead the feedback; the missing-tests hint is
    // appended only as secondary context. "no test() calls found" stands
    // alone only for a CLEAN execution that simply registered no tests.
    const fb =
      exec.status !== "ok"
        ? [
            `sandbox ${exec.status}${exec.error === undefined ? "" : `: ${exec.error}`}`,
            ...(exec.tests.length === 0
              ? [
                  'no test() calls found; add test("name", () => { expect(...) }) calls per the spec',
                ]
              : []),
            "fix and return the FULL file(s) again",
          ].join("\n")
        : exec.tests.length === 0
          ? 'no test() calls found; add test("name", () => { expect(...) }) calls per the spec, and return the FULL file(s) again'
          : formatSandboxFeedback(exec);
    iterations.push({
      n,
      llmMs: iterLlmMs,
      explanation: contract.explanation,
      files: contract.files,
      typecheck: check,
      transpileMs: tr.ms,
      sandbox: exec,
      feedback: fb,
    });
    feedbackHistory.push({ role: "user", content: fb });
  }

  return finish("max_iterations");
}

// v5 H8: the public entry — an unexpected throw from any seam carries the
// iterations completed so far as the partial transcript (AgentLoopError) so
// the HTTP layer can finalize the record before answering 500.
export async function runAgent(deps: AgentLoopDeps): Promise<AgentRunResult> {
  const iterations: AgentIteration[] = [];
  try {
    return await runLoop(deps, iterations);
  } catch (err) {
    throw new AgentLoopError(iterations, err);
  }
}
