import { describe, expect, it } from "vitest";
import {
  scanMaxNestingDepth,
  WASM_GUEST_LIMITS,
  WasmGuestTooComplexError,
  assertGuestWithinWasmLimits,
} from "../src/typecheck-wasm/guard";

describe("scanMaxNestingDepth", () => {
  it("counts code nesting across bracket kinds", () => {
    expect(scanMaxNestingDepth("f(g([{}]))")).toBe(4);
  });

  it("ignores brackets in strings, comments, and templates", () => {
    const src = `const s = "((("; // (((
/* [[[" */
const t = \`{{{ \`;
const u = { a: 1 };`;
    expect(scanMaxNestingDepth(src)).toBe(1);
  });

  it("scans ${} interpolation holes as code and returns to the template", () => {
    const src = "const t = `x${ g([1]) }y`;";
    expect(scanMaxNestingDepth(src)).toBe(3);
  });

  it("does not go negative on unbalanced closes", () => {
    expect(scanMaxNestingDepth(")))]")).toBe(0);
  });
});

describe("assertGuestWithinWasmLimits", () => {
  it("accepts a normal guest and the calibrated depth budget", () => {
    const ok = "const x = " + "(".repeat(WASM_GUEST_LIMITS.maxNestingDepth - 1) + "1" + ")".repeat(WASM_GUEST_LIMITS.maxNestingDepth - 1) + ";";
    expect(() => assertGuestWithinWasmLimits("/guest.ts", ok)).not.toThrow();
  });

  it("rejects nesting past the calibrated limit with ts-js guidance", () => {
    const deep = "const x = " + "(".repeat(WASM_GUEST_LIMITS.maxNestingDepth + 1) + "1" + ")".repeat(WASM_GUEST_LIMITS.maxNestingDepth + 1) + ";";
    expect(() => assertGuestWithinWasmLimits("/guest.ts", deep)).toThrow(WasmGuestTooComplexError);
    expect(() => assertGuestWithinWasmLimits("/guest.ts", deep)).toThrow(/TSCHECKER=ts-js/);
  });

  it("rejects oversized sources by bytes", () => {
    const big = "const x = \"" + "a".repeat(WASM_GUEST_LIMITS.maxSourceBytes) + "\";";
    expect(() => assertGuestWithinWasmLimits("/guest.ts", big)).toThrow(/bytes/);
  });
});
