import { describe, expect, it } from "vitest";
import {
  MAX_SHELL_COMMANDS,
  MAX_SHELL_VFS_BYTES,
  runShellSession,
  ShellBadRequestError,
  type ShellDeps,
} from "../src/shell/shell";
import type { ExecuteResult, SandboxRuntime } from "../src/sandbox/runtime";
import type { TranspileResult } from "../src/typecheck/transpile";
import type { TypecheckResult, Typechecker } from "../src/typecheck/index";

// Pure shell session with stub seams ONLY (small-module discipline — the
// typecheck/sandbox imports above are type-only and erased, and shell.ts has
// zero runtime imports). Real workerd behavior (route wiring, identity
// flips, QuickJS guardrails, HTTP 400s) is proven by
// scripts/test-shell-live.mjs.

const OK_CHECK: TypecheckResult = { ok: true, diagnostics: [], ms: 1 };
// Stub data is PROGRAMMED, not computed — the diagnostic below mirrors the
// exact shape the real checker emits (1-based line/column).
const BAD_CHECK: TypecheckResult = {
  ok: false,
  diagnostics: [
    {
      code: 2322,
      message: "Type 'string' is not assignable to type 'number'.",
      line: 2,
      column: 9,
      file: "/guest.ts",
      category: "Error",
    },
  ],
  ms: 1,
};

class StubTypechecker implements Typechecker {
  readonly id = "stub";
  calls = 0;
  lastFiles: Map<string, string> | null = null;
  constructor(private readonly result: TypecheckResult) {}
  async check(files: Map<string, string>): Promise<TypecheckResult> {
    this.calls += 1;
    this.lastFiles = files;
    return this.result;
  }
}

class StubSandbox implements SandboxRuntime {
  calls: string[] = [];
  constructor(private readonly result: ExecuteResult) {}
  async execute(code: string): Promise<ExecuteResult> {
    this.calls.push(code);
    return this.result;
  }
}

const transpile = (source: string): TranspileResult => ({ js: source, diagnostics: [], ms: 1 });

const deps = (over: Partial<ShellDeps> = {}): ShellDeps => ({
  typechecker: new StubTypechecker(OK_CHECK),
  sandbox: new StubSandbox({ status: "ok", logs: [], tests: [], ms: 2 }),
  transpile,
  ...over,
});

describe("shell session ops", () => {
  it("write -> cat -> ls -> rm -> cat roundtrips over the per-request VFS", async () => {
    const session = await runShellSession(
      {
        commands: [
          { op: "write", args: { file: "guest.ts", content: "const x = 1;\n" } },
          { op: "cat", args: { file: "guest.ts" } },
          { op: "ls" },
          { op: "rm", args: { file: "guest.ts" } },
          { op: "cat", args: { file: "guest.ts" } },
        ],
      },
      deps(),
    );
    expect(session.haltedAt).toBeUndefined();
    expect(session.results.map((r) => r.op)).toEqual(["write", "cat", "ls", "rm", "cat"]);
    expect(session.results[0].result).toEqual({ name: "/guest.ts", bytes: 13, totalBytes: 13 });
    expect(session.results[1].result).toEqual({ name: "/guest.ts", content: "const x = 1;\n", bytes: 13 });
    expect(session.results[2].result).toEqual({ files: [{ name: "/guest.ts", bytes: 13 }] });
    expect(session.results[3].result).toEqual({ name: "/guest.ts", removed: true, totalBytes: 0 });
    expect(session.results[4].ok).toBe(false);
    expect(session.results[4].result).toMatchObject({ code: "not_found", name: "/guest.ts" });
    expect(session.results.every((r) => typeof r.ms === "number" && r.ms >= 0)).toBe(true);
  });

  it("names are normalized to a leading slash and seed files land before op writes", async () => {
    const session = await runShellSession(
      {
        files: { "seed.ts": "const seed = 1;\n" },
        commands: [
          { op: "write", args: { file: "/guest.ts", content: "const y = 2;\n" } },
          { op: "cat", args: { file: "guest.ts" } },
          { op: "ls" },
        ],
      },
      deps(),
    );
    expect(session.results[1].result).toEqual({ name: "/guest.ts", content: "const y = 2;\n", bytes: 13 });
    expect(session.results[2].result).toEqual({
      files: [
        { name: "/seed.ts", bytes: 16 },
        { name: "/guest.ts", bytes: 13 },
      ],
    });
  });

  it("write upserts: one ls entry, cat returns the latest content", async () => {
    const session = await runShellSession(
      {
        commands: [
          { op: "write", args: { file: "g.ts", content: "first\n" } },
          { op: "write", args: { file: "g.ts", content: "second!\n" } },
          { op: "ls" },
          { op: "cat", args: { file: "g.ts" } },
        ],
      },
      deps(),
    );
    expect(session.results[2].result).toEqual({ files: [{ name: "/g.ts", bytes: 8 }] });
    expect(session.results[3].result).toEqual({ name: "/g.ts", content: "second!\n", bytes: 8 });
    expect(session.results[1].result).toMatchObject({ totalBytes: 8 });
  });

  it("unknown op is an op-level failure and the session continues", async () => {
    const session = await runShellSession(
      { commands: [{ op: "frobnicate" }, { op: "ls" }] },
      deps(),
    );
    expect(session.haltedAt).toBeUndefined();
    expect(session.results[0]).toMatchObject({
      op: "frobnicate",
      ok: false,
      result: { code: "unknown_op", op: "frobnicate" },
    });
    expect(session.results[1].ok).toBe(true);
  });

  it("cat/rm on a missing file fail with not_found and list available names", async () => {
    const session = await runShellSession(
      {
        files: { "seed.ts": "const seed = 1;\n" },
        commands: [
          { op: "cat", args: { file: "missing.ts" } },
          { op: "rm", args: { file: "missing.ts" } },
        ],
      },
      deps(),
    );
    expect(session.results[0]).toMatchObject({
      ok: false,
      result: { code: "not_found", name: "/missing.ts", available: ["/seed.ts"] },
    });
    expect(session.results[1]).toMatchObject({
      ok: false,
      result: { code: "not_found", name: "/missing.ts", available: ["/seed.ts"] },
    });
  });

  it("typecheck passes the session VFS to the checker and mirrors its verdict inline", async () => {
    const checker = new StubTypechecker(BAD_CHECK);
    const session = await runShellSession(
      {
        commands: [
          { op: "write", args: { file: "guest.ts", content: "const slug: number = 'x';\n" } },
          { op: "typecheck" },
        ],
      },
      deps({ typechecker: checker }),
    );
    expect(checker.calls).toBe(1);
    expect(checker.lastFiles?.get("/guest.ts")).toBe("const slug: number = 'x';\n");
    expect(session.results[1].ok).toBe(false);
    expect(session.results[1].result).toEqual(BAD_CHECK);
    expect((session.results[1].result as TypecheckResult).diagnostics[0]).toMatchObject({
      code: 2322,
      line: 2,
      column: 9,
    });
  });

  it("typecheck on an empty VFS fails with the empty code without calling the checker", async () => {
    const checker = new StubTypechecker(OK_CHECK);
    const session = await runShellSession({ commands: [{ op: "typecheck" }] }, deps({ typechecker: checker }));
    expect(session.results[0].ok).toBe(false);
    expect(session.results[0].result).toMatchObject({ code: "empty" });
    expect(checker.calls).toBe(0);
  });

  it("run transpiles and executes the file, returning transpileMs + the sandbox result", async () => {
    const sandbox = new StubSandbox({
      status: "ok",
      logs: [],
      tests: [{ name: "t", passed: true }],
      ms: 2,
    });
    const session = await runShellSession(
      {
        commands: [
          { op: "write", args: { file: "guest.ts", content: "const a = 1;\n" } },
          { op: "run", args: { file: "guest.ts" } },
        ],
      },
      deps({ sandbox }),
    );
    expect(session.results[1].ok).toBe(true);
    expect(session.results[1].result).toEqual({
      transpileMs: 1,
      sandbox: { status: "ok", logs: [], tests: [{ name: "t", passed: true }], ms: 2 },
    });
    expect(sandbox.calls).toEqual(["const a = 1;\n"]);
  });

  it("run reports a failed execution as ok:false with the ExecuteResult inline", async () => {
    const sandbox = new StubSandbox({ status: "timeout", logs: [], tests: [], error: "deadline", ms: 5 });
    const session = await runShellSession(
      {
        commands: [
          { op: "write", args: { file: "loop.ts", content: "while(true) {}\n" } },
          { op: "run", args: { file: "loop.ts" } },
        ],
      },
      deps({ sandbox }),
    );
    expect(session.results[1].ok).toBe(false);
    expect((session.results[1].result as { sandbox: ExecuteResult }).sandbox).toEqual({
      status: "timeout",
      logs: [],
      tests: [],
      error: "deadline",
      ms: 5,
    });
  });

  it("run on a missing file fails not_found without touching the sandbox", async () => {
    const sandbox = new StubSandbox({ status: "ok", logs: [], tests: [], ms: 1 });
    const session = await runShellSession(
      { commands: [{ op: "run", args: { file: "nope.ts" } }] },
      deps({ sandbox }),
    );
    expect(session.results[0].ok).toBe(false);
    expect(session.results[0].result).toMatchObject({ code: "not_found", name: "/nope.ts" });
    expect(sandbox.calls).toEqual([]);
  });

  it("stopOnError halts at the first failed op; the default false runs on", async () => {
    const haltedDeps = deps({ typechecker: new StubTypechecker(BAD_CHECK) });
    const halted = await runShellSession(
      {
        stopOnError: true,
        commands: [
          { op: "write", args: { file: "guest.ts", content: "broken" } },
          { op: "typecheck" },
          { op: "run", args: { file: "guest.ts" } },
        ],
      },
      haltedDeps,
    );
    expect(halted.haltedAt).toBe(1);
    expect(halted.results.length).toBe(2);
    expect(halted.results[1].op).toBe("typecheck");

    const runsOn = await runShellSession(
      {
        commands: [
          { op: "write", args: { file: "guest.ts", content: "broken" } },
          { op: "typecheck" },
          { op: "run", args: { file: "guest.ts" } },
        ],
      },
      deps({ typechecker: new StubTypechecker(BAD_CHECK) }),
    );
    expect(runsOn.haltedAt).toBeUndefined();
    expect(runsOn.results.length).toBe(3);
    expect(runsOn.results[2].op).toBe("run");
  });

  it("more than MAX_SHELL_COMMANDS commands (or none) is a request-shape error", async () => {
    await expect(
      runShellSession(
        { commands: Array.from({ length: MAX_SHELL_COMMANDS + 1 }, () => ({ op: "ls" })) },
        deps(),
      ),
    ).rejects.toBeInstanceOf(ShellBadRequestError);
    await expect(runShellSession({ commands: [] }, deps())).rejects.toBeInstanceOf(
      ShellBadRequestError,
    );
    await expect(runShellSession({ commands: "ls" }, deps())).rejects.toBeInstanceOf(
      ShellBadRequestError,
    );
  });

  it("write pushing the VFS past 1 MiB fails vfs_full and leaves the VFS unchanged", async () => {
    const session = await runShellSession(
      {
        files: { "big.ts": "x".repeat(MAX_SHELL_VFS_BYTES) },
        commands: [
          { op: "write", args: { file: "small.ts", content: "y" } },
          { op: "cat", args: { file: "big.ts" } },
        ],
      },
      deps(),
    );
    expect(session.results[0].ok).toBe(false);
    expect(session.results[0].result).toMatchObject({
      code: "vfs_full",
      name: "/small.ts",
      attemptedBytes: 1,
      capBytes: MAX_SHELL_VFS_BYTES,
    });
    expect(session.results[1].ok).toBe(true);
    expect((session.results[1].result as { bytes: number }).bytes).toBe(MAX_SHELL_VFS_BYTES);
  });

  it("seed files over the 1 MiB cap are a request-shape error", async () => {
    await expect(
      runShellSession(
        { commands: [{ op: "ls" }], files: { "big.ts": "x".repeat(MAX_SHELL_VFS_BYTES + 1) } },
        deps(),
      ),
    ).rejects.toBeInstanceOf(ShellBadRequestError);
  });
});
