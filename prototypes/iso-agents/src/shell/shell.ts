// Agent shell session (SHELL-01/02): a stateless tool-protocol run over a
// per-request Map VFS. PURE module — ZERO runtime imports; typechecker,
// sandbox and transpile enter as injected deps (mirrors src/agent/loop.ts),
// so vitest specs can stub them without dragging the 8.8 MB typescript or
// the quickjs graph into the worker transport (Phase 2 lesson). The HTTP
// layer lives in src/index.ts, which wires the real per-request primitives.
//
// SHELL-02 guardrails are inherited BY CONSTRUCTION, not re-implemented:
// the run op calls the injected SandboxRuntime (deadline interrupt, memory/
// stack limits, async-usage gate, source-size gate, fresh single-use
// context, console shim — all inside QuickJsSandboxRuntime) and the
// typecheck op calls the injected Typechecker (typecheck-gate semantics:
// its ok:false verdict feeds back as data, it never blocks the session
// unless stopOnError says so). There is no eval path anywhere on this
// module — gate:no-host-eval stays green by inspection and by grep.
import type { SandboxRuntime } from "../sandbox/runtime";
import type { Typechecker } from "../typecheck/index";
import type { TranspileResult } from "../typecheck/transpile";

export const MAX_SHELL_COMMANDS = 32;
export const MAX_SHELL_VFS_BYTES = 1048576; // 1 MiB total across all files

export interface ShellSessionInput {
  commands: unknown;
  files?: unknown;
  stopOnError?: unknown;
}

export interface ShellDeps {
  typechecker: Typechecker;
  sandbox: SandboxRuntime;
  transpile: (source: string) => TranspileResult;
}

export interface ShellOpResult {
  op: string;
  ok: boolean;
  result: unknown; // JSON-serializable by construction (checked records / TypecheckResult / ExecuteResult)
  ms: number;
}

export interface ShellSessionResult {
  results: ShellOpResult[];
  // Index of the ok:false op that halted the session (stopOnError only).
  // Present iff halted; results.length === haltedAt + 1.
  haltedAt?: number;
}

// Request-shape violations (the route maps this to 400 bad_request). Per-op
// failures (unknown op, missing file, vfs_full, empty, failed check/run) are
// NEVER this error — they are ok:false entries in the session result.
export class ShellBadRequestError extends Error {}

function normalizeName(name: string): string {
  // Identical normalization to handleTypecheck / the loop's rootFiles:
  // bare names are rooted so diagnostics always report "/guest.ts".
  return name.startsWith("/") ? name : `/${name}`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

// Seed VFS from the optional `files` — same validation shape as /typecheck's
// files: object, string values, total under the cap. 400 material.
function buildSeedVfs(files: unknown): Map<string, string> {
  if (files === undefined) {
    return new Map();
  }
  if (!isRecord(files)) {
    throw new ShellBadRequestError("files must be an object of {name: content}");
  }
  const vfs = new Map<string, string>();
  let total = 0;
  for (const [name, content] of Object.entries(files)) {
    if (typeof content !== "string") {
      throw new ShellBadRequestError(`files.${name} must be a string`);
    }
    total += content.length;
    if (total > MAX_SHELL_VFS_BYTES) {
      throw new ShellBadRequestError(`seed files exceed the ${MAX_SHELL_VFS_BYTES}-byte VFS cap`);
    }
    vfs.set(normalizeName(name), content);
  }
  return vfs;
}

// Byte accounting uses string length (UTF-16 code units) — guests are ASCII
// in practice; the cap is a coarse prototype bound, not a precision claim.
function totalBytes(vfs: Map<string, string>): number {
  let total = 0;
  for (const content of vfs.values()) {
    total += content.length;
  }
  return total;
}

function requireFileArg(args: unknown, op: string): string {
  if (!isRecord(args) || typeof args.file !== "string") {
    throw new ShellBadRequestError(`op "${op}" requires args: {file: string}`);
  }
  return args.file;
}

export async function runShellSession(
  input: ShellSessionInput,
  deps: ShellDeps,
): Promise<ShellSessionResult> {
  if (!Array.isArray(input.commands) || input.commands.length === 0) {
    throw new ShellBadRequestError("commands must be a non-empty array of {op, args?}");
  }
  if (input.commands.length > MAX_SHELL_COMMANDS) {
    throw new ShellBadRequestError(`commands is limited to ${MAX_SHELL_COMMANDS} per session`);
  }
  if (input.stopOnError !== undefined && typeof input.stopOnError !== "boolean") {
    throw new ShellBadRequestError("stopOnError must be a boolean");
  }
  const stopOnError = input.stopOnError === true; // default false (CONTEXT)
  const vfs = buildSeedVfs(input.files);
  const results: ShellOpResult[] = [];
  let haltedAt: number | undefined;

  for (let index = 0; index < input.commands.length; index++) {
    const command = input.commands[index];
    if (!isRecord(command) || typeof command.op !== "string") {
      throw new ShellBadRequestError(`commands[${index}] must be an object with a string op`);
    }
    const t0 = performance.now();
    let ok = true;
    let result: unknown;
    switch (command.op) {
      case "ls": {
        result = {
          files: [...vfs.entries()].map(([name, content]) => ({ name, bytes: content.length })),
        };
        break;
      }
      case "cat": {
        const name = normalizeName(requireFileArg(command.args, "cat"));
        const content = vfs.get(name);
        if (content === undefined) {
          ok = false;
          result = { code: "not_found", name, available: [...vfs.keys()] };
        } else {
          result = { name, content, bytes: content.length };
        }
        break;
      }
      case "write": {
        if (
          !isRecord(command.args) ||
          typeof command.args.file !== "string" ||
          typeof command.args.content !== "string"
        ) {
          throw new ShellBadRequestError('op "write" requires args: {file: string, content: string}');
        }
        const name = normalizeName(command.args.file);
        const content = command.args.content;
        // Upsert accounting: bytes of the file being replaced do not count
        // toward the cap; a rejected write must NOT mutate the VFS.
        const others = totalBytes(vfs) - (vfs.get(name)?.length ?? 0);
        if (others + content.length > MAX_SHELL_VFS_BYTES) {
          ok = false;
          result = {
            code: "vfs_full",
            name,
            totalBytes: others,
            attemptedBytes: content.length,
            capBytes: MAX_SHELL_VFS_BYTES,
          };
        } else {
          vfs.set(name, content);
          result = { name, bytes: content.length, totalBytes: others + content.length };
        }
        break;
      }
      case "rm": {
        const name = normalizeName(requireFileArg(command.args, "rm"));
        if (!vfs.delete(name)) {
          ok = false;
          result = { code: "not_found", name, available: [...vfs.keys()] };
        } else {
          result = { name, removed: true, totalBytes: totalBytes(vfs) };
        }
        break;
      }
      case "typecheck": {
        // Gate semantics (SHELL-02): the session VFS is the check input; an
        // empty VFS is an op failure (mirrors /typecheck's >=1-entry rule).
        // A FAILING check is expected agent feedback — the full
        // TypecheckResult (1-based line/column diagnostics) rides inline.
        if (vfs.size === 0) {
          ok = false;
          result = { code: "empty", message: "typecheck needs at least one file in the session VFS" };
          break;
        }
        const check = await deps.typechecker.check(new Map(vfs));
        ok = check.ok;
        result = check;
        break;
      }
      case "run": {
        // Mirrors the loop pipeline (src/agent/loop.ts) minus the LLM:
        // transpile -> syntactic-error tripwire -> sandbox.execute. Every
        // guardrail lives inside the injected runtime; nothing is duplicated.
        const name = normalizeName(requireFileArg(command.args, "run"));
        const source = vfs.get(name);
        if (source === undefined) {
          ok = false;
          result = { code: "not_found", name, available: [...vfs.keys()] };
          break;
        }
        const tr = deps.transpile(source);
        const transpileErrors = tr.diagnostics.filter((d) => d.category === "Error");
        if (transpileErrors.length > 0) {
          ok = false;
          result = { code: "transpile_error", diagnostics: transpileErrors, transpileMs: tr.ms };
          break;
        }
        const exec = await deps.sandbox.execute(tr.js);
        // op.ok reflects the EXECUTION outcome (status !== "ok" fails the
        // op); failing TESTS inside a completed run stay ok — they are the
        // agent's feedback material, never dropped.
        ok = exec.status === "ok";
        result = { transpileMs: tr.ms, sandbox: exec };
        break;
      }
      default: {
        // An unknown op is an op-level failure, not a request rejection —
        // the session continues (stopOnError permitting).
        ok = false;
        result = { code: "unknown_op", op: command.op };
      }
    }
    results.push({ op: command.op, ok, result, ms: performance.now() - t0 });
    if (!ok && stopOnError) {
      haltedAt = index;
      break;
    }
  }

  return haltedAt === undefined ? { results } : { results, haltedAt };
}
