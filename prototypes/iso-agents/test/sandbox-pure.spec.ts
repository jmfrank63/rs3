import { describe, expect, it } from "vitest";
import { ASYNC_SOURCE_PATTERN, findAsyncUsage } from "../src/sandbox/gate";
import { HARNESS_SOURCE, makeHarness, type HarnessRecord } from "../src/sandbox/harness";
import { SANDBOX_IDENTITY, getSandboxIdentity, markSandboxLoaded } from "../src/sandbox/identity";
import {
  DEFAULT_MEMORY_LIMIT_MB,
  DEFAULT_STACK_KB,
  DEFAULT_TIMEOUT_MS,
  MAX_GUEST_SOURCE_BYTES,
  MAX_STACK_KB,
  parseLimits,
} from "../src/sandbox/limits";

// Pure sandbox pieces ONLY: this spec must never import src/sandbox/runtime.ts
// (its module graph pulls quickjs-emscripten-core into the vitest worker
// transport — the Phase 2 lesson). Runtime behavior is proven by
// scripts/test-sandbox-live.mjs (03-02) against real workerd.

describe("sandbox limits", () => {
  it("defaults: 2000ms timeout, 64MiB memory, 64KiB stack", () => {
    const limits = parseLimits({});
    expect(limits.timeoutMs).toBe(DEFAULT_TIMEOUT_MS);
    expect(limits.timeoutMs).toBe(2000);
    expect(limits.memoryLimitBytes).toBe(DEFAULT_MEMORY_LIMIT_MB * 1024 * 1024);
    expect(limits.memoryLimitBytes).toBe(67108864);
    expect(limits.stackBytes).toBe(DEFAULT_STACK_KB * 1024);
    expect(limits.stackBytes).toBe(65536);
  });

  it("env overrides parse; stack hard-caps at 128KiB", () => {
    const limits = parseLimits({
      SANDBOX_TIMEOUT_MS: "500",
      SANDBOX_MEMORY_LIMIT_MB: "32",
      SANDBOX_STACK_KB: "512",
    });
    expect(limits.timeoutMs).toBe(500);
    expect(limits.memoryLimitBytes).toBe(32 * 1024 * 1024);
    expect(limits.stackBytes).toBe(MAX_STACK_KB * 1024);
    expect(limits.stackBytes).toBe(131072);
  });

  it("garbage env values fall back to defaults", () => {
    const limits = parseLimits({
      SANDBOX_TIMEOUT_MS: "abc",
      SANDBOX_MEMORY_LIMIT_MB: "-4",
      SANDBOX_STACK_KB: "0",
    });
    expect(limits.timeoutMs).toBe(2000);
    expect(limits.memoryLimitBytes).toBe(64 * 1024 * 1024);
    expect(limits.stackBytes).toBe(64 * 1024);
  });
});

describe("async source gate", () => {
  it("flags async, await, and Promise tokens", () => {
    expect(findAsyncUsage("async function f() {}")).toBe("async");
    expect(findAsyncUsage("await Promise.resolve()")).toBe("await");
    expect(findAsyncUsage("new Promise(function (resolve) { resolve(1); })")).toBe("Promise");
    expect(ASYNC_SOURCE_PATTERN.test("function f() { return 1; }")).toBe(false);
  });

  it("word boundaries: identifiers containing the tokens are not flagged", () => {
    expect(findAsyncUsage("const myPromiseValue = 1;")).toBe(null);
    expect(findAsyncUsage("unawaited = 1;")).toBe(null);
  });

  it("guest source cap is 20480 bytes", () => {
    expect(MAX_GUEST_SOURCE_BYTES).toBe(20480);
  });
});

describe("guest harness source", () => {
  it("locks test(), chainable expect(), and assert() onto the guest global", () => {
    // H5: defineProperty-locked (non-writable, non-configurable), not the
    // old overwritable `globalThis.test = ...` assignments.
    expect(HARNESS_SOURCE.includes("makeHarness(globalObject)")).toBe(true);
    expect(HARNESS_SOURCE.endsWith("(globalThis);\n})();")).toBe(true);
    expect(HARNESS_SOURCE.includes('lock("test", testFn)')).toBe(true);
    expect(HARNESS_SOURCE.includes('lock("expect", expectFn)')).toBe(true);
    expect(HARNESS_SOURCE.includes('lock("assert", assertFn)')).toBe(true);
    expect(HARNESS_SOURCE.includes("writable: false")).toBe(true);
    expect(HARNESS_SOURCE.includes("configurable: false")).toBe(true);
  });

  it("shims esbuild's keepNames helper (the bundled body carries __name decorators)", () => {
    expect(HARNESS_SOURCE.includes("var __name = function (target, value)")).toBe(true);
  });

  it("keeps the records array OFF the guest global (no __tests to fabricate)", () => {
    expect(HARNESS_SOURCE.includes("__tests")).toBe(false);
    expect(HARNESS_SOURCE.includes("return records")).toBe(true);
  });

  it("harness source is itself sync-only (passes the gate)", () => {
    expect(findAsyncUsage(HARNESS_SOURCE)).toBe(null);
  });
});

// H5 behavioral proofs against the REAL harness function (makeHarness is
// self-contained, so specs run it directly — workerd forbids dynamic
// codegen, so the SOURCE string itself is exercised live by
// scripts/test-sandbox-live.mjs instead).
describe("harness private test records (H5)", () => {
  const run = (): { global: Record<string, unknown>; records: HarnessRecord[] } => {
    const global: Record<string, unknown> = {};
    const records = makeHarness(global);
    return { global, records };
  };

  it("records real test results into the closure-private array", () => {
    const { global, records } = run();
    (global.test as (n: string, f: () => void) => void)("passing", () => {
      expect(1).toBe(1);
    });
    (global.test as (n: string, f: () => void) => void)("failing", () => {
      throw new RangeError("boom");
    });
    expect(records).toEqual([
      { name: "passing", passed: true },
      { name: "failing", passed: false, error: "RangeError: boom" },
    ]);
  });

  it("the records array is unreachable from the guest global object", () => {
    const { global } = run();
    expect(global.__tests).toBeUndefined();
    expect(Object.getOwnPropertyNames(global).sort()).toEqual(["assert", "expect", "test"]);
  });

  it("harness functions resist replacement (locked descriptors, silent no-op in sloppy code)", () => {
    const { global, records } = run();
    const desc = Object.getOwnPropertyDescriptor(global, "test");
    expect(desc?.writable).toBe(false);
    expect(desc?.configurable).toBe(false);
    // Strict-mode assignment attempts (the sandbox evals guests with
    // strict: true, and this transport is strict ESM too) throw a TypeError
    // instead of replacing the harness — the real test() survives either
    // way and no fabricated record can enter the private array.
    expect(() => {
      global.test = () => {
        records.push({ name: "fabricated", passed: true });
      };
    }).toThrow(TypeError);
    (global.test as (n: string, f: () => void) => void)("real", () => {});
    expect(records).toEqual([{ name: "real", passed: true }]);
    expect(() => Object.defineProperty(global, "test", { value: () => {} })).toThrow(TypeError);
  });
});

// H6 behavioral proofs: type-aware assertions — the old JSON.stringify
// equality made NaN "equal" anything that stringified identically and
// treated undefined as equal to a function; toBeCloseTo silently PASSED on
// NaN because `NaN > tolerance` is false.
describe("harness type-aware assertions (H6)", () => {
  const run = (): { global: Record<string, unknown>; records: HarnessRecord[] } => {
    const global: Record<string, unknown> = {};
    return { global, records: makeHarness(global) };
  };
  const expectOn = (global: Record<string, unknown>): (actual: unknown) => Record<string, (...a: unknown[]) => void> =>
    global.expect as never;

  it("expect(NaN).toBeCloseTo(42) fails (was a false pass)", () => {
    const { global, records } = run();
    expect(() => expectOn(global)(NaN).toBeCloseTo(42)).toThrow(/toBeCloseTo needs finite numbers/);
    expect(() => expectOn(global)(42).toBeCloseTo(NaN)).toThrow(/toBeCloseTo needs finite numbers/);
    expect(() => expectOn(global)(NaN).toBeCloseTo(NaN)).toThrow(/toBeCloseTo needs finite numbers/);
    expect(records).toEqual([]);
  });

  it("expect(NaN).toEqual(null) fails (was a false pass: identical JSON)", () => {
    const { global } = run();
    expect(() => expectOn(global)(NaN).toEqual(null)).toThrow(/expected null, got NaN/);
    expect(() => expectOn(global)(NaN).toEqual(NaN)).toThrow(/expected NaN, got NaN/);
    expect(() => expectOn(global)(null).toEqual(NaN)).toThrow(/expected NaN, got null/);
  });

  it("undefined is never structurally equal to a function (was a false pass)", () => {
    const { global } = run();
    expect(() => expectOn(global)(undefined).toEqual(() => 1)).toThrow();
    expect(() => expectOn(global)(() => 1).toEqual(undefined)).toThrow();
  });

  it("structural equality still passes honest data (arrays, nested objects, primitives)", () => {
    const { global } = run();
    const expectFn = expectOn(global);
    expectFn(1).toEqual(1);
    expectFn("a").toEqual("a");
    expectFn({ a: 1, b: [1, { c: "x" }] }).toEqual({ a: 1, b: [1, { c: "x" }] });
    expectFn([1, 2, 3]).toEqual([1, 2, 3]);
    expect(() => expectFn({ a: 1 }).toEqual({ a: 2 })).toThrow(/expected {"a":2}, got {"a":1}/);
    expect(() => expectFn([1, 2]).toEqual([1, 2, 3])).toThrow();
    expect(() => expectFn([1, 2, 3]).toEqual([1, 2])).toThrow();
  });

  it("toBeCloseTo still passes honest numbers and honors the tolerance", () => {
    const { global } = run();
    const expectFn = expectOn(global);
    expectFn(0.1 + 0.2).toBeCloseTo(0.3);
    expectFn(1.0).toBeCloseTo(1.0001, 0.001);
    expect(() => expectFn(1.0).toBeCloseTo(1.1)).toThrow(/expected ~1.1, got 1/);
    expect(() => expectFn(1.0).toBeCloseTo(1.1, -1)).toThrow(/tolerance/);
    expect(() => expectFn("1").toBeCloseTo(1)).toThrow(/toBeCloseTo needs finite numbers/);
  });

  it("Infinity fails toBeCloseTo like NaN (finite operands only)", () => {
    const { global } = run();
    expect(() => expectOn(global)(Infinity).toBeCloseTo(42)).toThrow(/finite numbers/);
  });
});

describe("sandbox identity", () => {
  it("reports unloaded before the first execution", () => {
    expect(getSandboxIdentity()).toBe("unloaded");
  });

  it("flips to the variant identity once the module has loaded", () => {
    markSandboxLoaded();
    expect(getSandboxIdentity()).toBe(SANDBOX_IDENTITY);
    expect(getSandboxIdentity()).toBe("quickjs-wasmfile-release-sync@0.32.0");
  });
});
