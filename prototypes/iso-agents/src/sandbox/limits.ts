// Sandbox guardrail limits (SAND-02), parsed from env once per execution.
// The stack hard-cap is a CODE-LEVEL invariant: >128KiB overruns the native
// WASM stack before QuickJS's JS-level check — the host gets an uncatchable
// RangeError and context.dispose() then ABORTS (runtime poison, 03-RESEARCH
// rows 7/8). 64KiB default is the verified-safe zone; never raise the cap.
export interface SandboxLimits {
  timeoutMs: number;
  memoryLimitBytes: number;
  stackBytes: number;
}

export interface SandboxEnv {
  SANDBOX_TIMEOUT_MS?: string;
  SANDBOX_MEMORY_LIMIT_MB?: string;
  SANDBOX_STACK_KB?: string;
}

export const DEFAULT_TIMEOUT_MS = 2000;
export const DEFAULT_MEMORY_LIMIT_MB = 64;
export const DEFAULT_STACK_KB = 64;
export const MAX_STACK_KB = 128;
export const MAX_GUEST_SOURCE_BYTES = 20480;

const KIB = 1024;
const MIB = 1024 * 1024;

function parsePositiveInt(raw: string | undefined, fallback: number): number {
  if (raw === undefined) return fallback;
  const n = Number(raw);
  return Number.isInteger(n) && n > 0 ? n : fallback;
}

export function parseLimits(env: SandboxEnv): SandboxLimits {
  return {
    timeoutMs: parsePositiveInt(env.SANDBOX_TIMEOUT_MS, DEFAULT_TIMEOUT_MS),
    memoryLimitBytes:
      parsePositiveInt(env.SANDBOX_MEMORY_LIMIT_MB, DEFAULT_MEMORY_LIMIT_MB) * MIB,
    stackBytes:
      Math.min(parsePositiveInt(env.SANDBOX_STACK_KB, DEFAULT_STACK_KB), MAX_STACK_KB) * KIB,
  };
}
