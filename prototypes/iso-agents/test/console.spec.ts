import { describe, expect, it } from "vitest";
import { fitLogArgs, jsonSafe, MAX_LOG_BYTES } from "../src/sandbox/console";

// v5 H9/H10 regressions: the sandbox's serialization + console-retention
// policy is pure host-side logic, so these run without the quickjs graph.

describe("jsonSafe (H9)", () => {
  it("stringifies BigInts (the 1n result that used to 500 the HTTP layer)", () => {
    expect(jsonSafe(1n)).toBe("1");
    expect(jsonSafe({ big: 9007199254740993n, small: 1 })).toEqual({ big: "9007199254740993n".slice(0, -1), small: 1 });
  });

  it("cuts cycles with the [circular] marker but keeps shared acyclic references", () => {
    const cyclic: Record<string, unknown> = { name: "root" };
    cyclic.self = cyclic;
    expect(jsonSafe(cyclic)).toEqual({ name: "root", self: "[circular]" });

    const shared = { v: 1 };
    const dag = { a: shared, b: shared };
    expect(jsonSafe(dag)).toEqual({ a: { v: 1 }, b: { v: 1 } });

    const nestedCycle: Record<string, unknown> = { list: [] };
    (nestedCycle.list as unknown[]).push(nestedCycle);
    expect(jsonSafe(nestedCycle)).toEqual({ list: ["[circular]"] });
  });

  it("passes primitives, arrays, and plain data through untouched", () => {
    expect(jsonSafe(undefined)).toBeUndefined();
    expect(jsonSafe(null)).toBeNull();
    expect(jsonSafe(42)).toBe(42);
    expect(jsonSafe("x")).toBe("x");
    expect(jsonSafe([1, "two", { three: 3 }])).toEqual([1, "two", { three: 3 }]);
    expect(jsonSafe(true)).toBe(true);
  });

  it("output is always JSON.stringify-able (the end-to-end contract)", () => {
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;
    const weird = { n: 1n, c: cyclic, f: () => 1, u: undefined };
    expect(() => JSON.stringify(jsonSafe(weird))).not.toThrow();
  });
});

describe("fitLogArgs (H10)", () => {
  it("bounds a 1 MB string to the cap (the retained-entry regression)", () => {
    const huge = "x".repeat(1_000_000);
    const fitted = fitLogArgs([huge], MAX_LOG_BYTES);
    expect(fitted.args[0]).toBeTypeOf("string");
    expect((fitted.args[0] as string).length).toBeLessThanOrEqual(MAX_LOG_BYTES - 1);
    expect(fitted.bytes).toBeLessThanOrEqual(MAX_LOG_BYTES);
  });

  it("collapses oversized non-strings to a bounded marker", () => {
    const fitted = fitLogArgs([{ blob: "y".repeat(100_000) }], MAX_LOG_BYTES);
    expect(fitted.args).toEqual(["[unserializable]"]);
    expect(fitted.bytes).toBeLessThanOrEqual(MAX_LOG_BYTES);
  });

  it("keeps honest small entries untouched", () => {
    const fitted = fitLogArgs(["answer", 42, { a: [1, 2] }], MAX_LOG_BYTES);
    expect(fitted.args).toEqual(["answer", 42, { a: [1, 2] }]);
    expect(fitted.bytes).toBe(7 + 3 + 12); // JSON length + 1 per arg
  });

  it("stops retaining once the budget is spent", () => {
    const fitted = fitLogArgs(["first", "second", "third"], 10);
    // "first" costs 6 of 10; 4 remain -> a 3-char slice + separator.
    expect(fitted.args).toEqual(["first", "sec"]);
    expect(fitted.bytes).toBeLessThanOrEqual(10);
  });

  it("zero budget retains nothing", () => {
    expect(fitLogArgs(["anything"], 0)).toEqual({ args: [], bytes: 0 });
  });
});
