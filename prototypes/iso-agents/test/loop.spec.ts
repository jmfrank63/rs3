import { describe, expect, it } from "vitest";
import { AgentLoopError, runAgent, type AgentLoopDeps } from "../src/agent/loop";
import { LlmError, type LlmCompletion, type LlmMessage, type LlmProvider } from "../src/agent/provider";
import type { ExecuteResult, SandboxRuntime } from "../src/sandbox/runtime";
import type { TranspileResult } from "../src/typecheck/transpile";
import type { Typechecker, TypecheckResult } from "../src/typecheck/index";

// Loop logic with stub seams ONLY (small-module discipline — no typescript /
// quickjs in this graph: the typecheck/sandbox imports above are type-only
// and erased). Real gates are proven live by scripts/test-agent-live.mjs.

const GREEN = 'function answer(): number {\n  return 42;\n}\ntest("answer", () => { expect(answer()).toEqual(42); });\n';
const BROKEN = 'function answer(): string {\n  return 42;\n}\ntest("answer", () => { expect(answer()).toEqual("42"); });\n';

const contract = (code: string): string =>
  JSON.stringify({ explanation: "stub draft", files: { "guest.ts": code } });

class StubProvider implements LlmProvider {
  readonly id = "stub";
  calls = 0;
  constructor(private readonly texts: string[]) {}
  complete(_messages: LlmMessage[]): Promise<LlmCompletion> {
    const text = this.texts[Math.min(this.calls, this.texts.length - 1)];
    this.calls += 1;
    return Promise.resolve({ text });
  }
}

class FailingProvider implements LlmProvider {
  readonly id = "stub-failing";
  calls = 0;
  complete(_messages: LlmMessage[]): Promise<LlmCompletion> {
    this.calls += 1;
    return Promise.reject(new LlmError("llm down", false));
  }
}

// H4: an awaited stage that overruns the run budget — the provider resolves
// AFTER the deadline has passed.
class SlowProvider implements LlmProvider {
  readonly id = "stub-slow";
  calls = 0;
  constructor(
    private readonly text: string,
    private readonly delayMs: number,
  ) {}
  complete(_messages: LlmMessage[], _opts?: { deadlineMs?: number }): Promise<LlmCompletion> {
    this.calls += 1;
    return new Promise((resolve) => {
      setTimeout(() => resolve({ text: this.text }), this.delayMs);
    });
  }
}

class StubTypechecker implements Typechecker {
  readonly id = "stub";
  private i = 0;
  constructor(private readonly results: TypecheckResult[]) {}
  async check(_files: Map<string, string>): Promise<TypecheckResult> {
    const result = this.results[Math.min(this.i, this.results.length - 1)];
    this.i += 1;
    return result;
  }
}

class StubSandbox implements SandboxRuntime {
  calls: string[] = [];
  private i = 0;
  constructor(private readonly results: ExecuteResult[]) {}
  async execute(code: string): Promise<ExecuteResult> {
    this.calls.push(code);
    const result = this.results[Math.min(this.i, this.results.length - 1)];
    this.i += 1;
    return result;
  }
}

const transpile = (source: string): TranspileResult => ({ js: source, diagnostics: [], ms: 1 });

const OK_CHECK: TypecheckResult = { ok: true, diagnostics: [], ms: 2 };
// Stub data is PROGRAMMED, not computed — line/column below are arbitrary
// values chosen to make the feedback-format assert exact.
const BAD_CHECK: TypecheckResult = {
  ok: false,
  diagnostics: [
    {
      code: 2322,
      message: "Type 'number' is not assignable to type 'string'.",
      line: 1,
      column: 28,
      file: "/guest.ts",
      category: "Error",
    },
  ],
  ms: 2,
};
const PASS: ExecuteResult = { status: "ok", logs: [], tests: [{ name: "answer", passed: true }], ms: 3 };
const FAIL: ExecuteResult = {
  status: "ok",
  logs: [{ level: "log", args: ["hmm"] }],
  tests: [{ name: "answer", passed: false, error: 'expected "42", got "42"' }],
  ms: 3,
};

function deps(overrides: Partial<AgentLoopDeps>): AgentLoopDeps {
  return {
    task: "task",
    spec: "spec",
    maxIterations: 5,
    runDeadlineMs: 25000,
    llm: new StubProvider([contract(GREEN)]),
    typechecker: new StubTypechecker([OK_CHECK]),
    sandbox: new StubSandbox([PASS]),
    transpile,
    ...overrides,
  };
}

describe("runAgent", () => {
  it("success path: one iteration, sandbox ran, full timings shape (OBS-01)", async () => {
    const sandbox = new StubSandbox([PASS]);
    const result = await runAgent(deps({ sandbox }));
    expect(result.status).toBe("success");
    expect(result.iterations.length).toBe(1);
    expect(result.iterations[0].sandbox).not.toBeNull();
    expect(result.iterations[0].feedback).toBeNull();
    expect(sandbox.calls.length).toBe(1);
    expect(Object.keys(result.timings).sort()).toEqual([
      "llmMs",
      "sandboxMs",
      "totalMs",
      "transpileMs",
      "typecheckMs",
    ]);
    // workerd's performance.now() ticks in whole ms, so the all-stub instant
    // path can legitimately total 0 — OBS-01 wants the duration recorded,
    // not positive (plan-review coarse-clock finding).
    expect(result.timings.totalMs).toBeGreaterThanOrEqual(0);
  });

  it("CORE-03: failing typecheck skips the sandbox and feeds diagnostics back", async () => {
    const sandbox = new StubSandbox([PASS]);
    const result = await runAgent(
      deps({
        llm: new StubProvider([contract(BROKEN), contract(GREEN)]),
        typechecker: new StubTypechecker([BAD_CHECK, OK_CHECK]),
        sandbox,
      }),
    );
    expect(result.status).toBe("success");
    expect(result.iterations.length).toBe(2);
    expect(result.iterations[0].sandbox).toBeNull();
    expect(result.iterations[0].typecheck?.ok).toBe(false);
    expect(result.iterations[0].feedback).toContain("/guest.ts:1:28 TS2322");
    expect(result.iterations[0].feedback).toContain("fix and return the FULL file(s) again");
    expect(sandbox.calls.length).toBe(1);
  });

  it("max_iterations: every iteration fails the gate and the loop stops at the cap", async () => {
    const sandbox = new StubSandbox([]);
    const result = await runAgent(
      deps({
        llm: new StubProvider([contract(BROKEN)]),
        typechecker: new StubTypechecker([BAD_CHECK]),
        sandbox,
        maxIterations: 3,
      }),
    );
    expect(result.status).toBe("max_iterations");
    expect(result.iterations.length).toBe(3);
    expect(sandbox.calls.length).toBe(0);
  });

  it("llm_error: a provider failure terminates the run with zero iterations", async () => {
    const llm = new FailingProvider();
    const result = await runAgent(deps({ llm }));
    expect(result.status).toBe("llm_error");
    expect(result.iterations.length).toBe(0);
    expect(llm.calls).toBe(1);
  });

  it("run_deadline: an expired deadline exits before the first LLM call", async () => {
    const llm = new StubProvider([contract(GREEN)]);
    const result = await runAgent(deps({ llm, runDeadlineMs: 0 }));
    expect(result.status).toBe("run_deadline");
    expect(result.iterations.length).toBe(0);
    expect(llm.calls).toBe(0);
  });

  it("H4: a provider call that overruns the budget terminates run_deadline before any gate runs", async () => {
    // 80ms provider sleep vs a 30ms run budget — the post-LLM-stage check
    // must fire; the old loop proceeded into check+execute regardless.
    const sandbox = new StubSandbox([PASS]);
    const result = await runAgent(
      deps({
        llm: new SlowProvider(contract(GREEN), 80),
        runDeadlineMs: 30,
        sandbox,
      }),
    );
    expect(result.status).toBe("run_deadline");
    expect(result.iterations.length).toBe(0);
    expect(sandbox.calls.length).toBe(0);
  });

  it("H4: a fully green iteration completed after the deadline is NEVER success", async () => {
    // The budget dies during the gates (slow sandbox stage): the green
    // verdict is still blocked — the completed iteration stays in the
    // transcript, but the terminal status is honest.
    class DelayedSandbox implements SandboxRuntime {
      calls: string[] = [];
      async execute(code: string): Promise<ExecuteResult> {
        this.calls.push(code);
        await new Promise((r) => setTimeout(r, 80));
        return PASS;
      }
    }
    const sandbox = new DelayedSandbox();
    const result = await runAgent(deps({ runDeadlineMs: 30, sandbox }));
    expect(result.status).toBe("run_deadline");
    expect(result.iterations.length).toBe(1);
    expect(result.iterations[0].sandbox).not.toBeNull();
    expect(result.iterations[0].sandbox?.tests.every((t) => t.passed)).toBe(true);
  });

  it("llm_malformed: one corrective retry, then the run terminates malformed", async () => {
    const llm = new StubProvider(["this is not json", "still not json"]);
    const result = await runAgent(deps({ llm }));
    expect(result.status).toBe("llm_malformed");
    expect(llm.calls).toBe(2);
    expect(result.iterations.length).toBe(1);
    expect(result.iterations[0].typecheck).toBeNull();
  });

  it("H8: an unexpected checker crash rejects with AgentLoopError carrying the partial transcript", async () => {
    class ThrowOnSecondCheck implements Typechecker {
      readonly id = "stub-throw-later";
      private calls = 0;
      async check(_files: Map<string, string>): Promise<TypecheckResult> {
        this.calls += 1;
        if (this.calls === 1) {
          return BAD_CHECK;
        }
        throw new RangeError("wasm trap: stack exhaustion");
      }
    }
    const err = await runAgent(
      deps({
        llm: new StubProvider([contract(BROKEN), contract(GREEN)]),
        typechecker: new ThrowOnSecondCheck(),
        sandbox: new StubSandbox([PASS]),
      }),
    ).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(AgentLoopError);
    const loopErr = err as AgentLoopError;
    // Iteration 1 (failed typecheck gate) completed BEFORE iteration 2's
    // checker crash — the partial transcript must survive for H8's
    // internal_error finalization.
    expect(loopErr.iterations.length).toBe(1);
    expect(loopErr.iterations[0].typecheck?.ok).toBe(false);
    expect(loopErr.message).toContain("stack exhaustion");
  });

  it("a test-less guest gets explicit feedback and the loop continues", async () => {
    const noTests: ExecuteResult = { status: "ok", logs: [], tests: [], ms: 1 };
    const result = await runAgent(
      deps({
        llm: new StubProvider([contract(GREEN), contract(GREEN)]),
        sandbox: new StubSandbox([noTests, PASS]),
      }),
    );
    expect(result.status).toBe("success");
    expect(result.iterations.length).toBe(2);
    expect(result.iterations[0].feedback).toContain("no test() calls found");
  });

  it("H11: a timed-out guest's feedback leads with the failure, not just missing-tests text", async () => {
    const timeoutExec: ExecuteResult = {
      status: "timeout",
      logs: [],
      tests: [],
      error: "execution exceeded the 2000ms sandbox deadline",
      ms: 1,
    };
    const result = await runAgent(
      deps({
        llm: new StubProvider([contract(GREEN), contract(GREEN)]),
        sandbox: new StubSandbox([timeoutExec, PASS]),
      }),
    );
    expect(result.status).toBe("success");
    const fb = result.iterations[0].feedback!;
    expect(fb.startsWith("sandbox timeout: execution exceeded the 2000ms sandbox deadline")).toBe(true);
    expect(fb).toContain("no test() calls found");
  });

  it("H11: a crashed guest with no tests surfaces status + error first", async () => {
    const crashExec: ExecuteResult = { status: "crash", logs: [], tests: [], error: "RangeError: boom", ms: 1 };
    const result = await runAgent(
      deps({
        llm: new StubProvider([contract(GREEN), contract(GREEN)]),
        sandbox: new StubSandbox([crashExec, PASS]),
      }),
    );
    expect(result.status).toBe("success");
    expect(result.iterations[0].feedback).toContain("sandbox crash: RangeError: boom");
  });

  it("H11: a failed status with registered tests keeps the full sandbox feedback", async () => {
    const timedOutWithTests: ExecuteResult = {
      status: "timeout",
      logs: [],
      tests: [{ name: "first", passed: true }],
      error: "execution exceeded the 2000ms sandbox deadline",
      ms: 1,
    };
    const result = await runAgent(
      deps({
        llm: new StubProvider([contract(GREEN), contract(GREEN)]),
        sandbox: new StubSandbox([timedOutWithTests, PASS]),
      }),
    );
    expect(result.status).toBe("success");
    const fb = result.iterations[0].feedback!;
    expect(fb).toContain("sandbox timeout: execution exceeded the 2000ms sandbox deadline");
    expect(fb).toContain("fix and return the FULL file(s) again");
  });
});
