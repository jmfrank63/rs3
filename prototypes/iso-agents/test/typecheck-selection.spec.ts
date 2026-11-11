import { describe, expect, it } from "vitest";
import {
  parseTSChecker,
  parseTSCheckerWasmRecycleAfter,
} from "../src/typecheck/selection";

// TSCHECKER selection parsing (WASM-03). Pure module — zero imports — so
// this spec never drags either checker graph into the vitest transport.
describe("parseTSChecker", () => {
  it("defaults to ts-js when env is undefined", () => {
    expect(parseTSChecker(undefined)).toBe("ts-js");
  });

  it("defaults to ts-js when TSCHECKER is unset or empty", () => {
    expect(parseTSChecker({})).toBe("ts-js");
    expect(parseTSChecker({ TSCHECKER: "" })).toBe("ts-js");
  });

  it("selects ts-js explicitly", () => {
    expect(parseTSChecker({ TSCHECKER: "ts-js" })).toBe("ts-js");
  });

  it("selects tsc-rs-wasm", () => {
    expect(parseTSChecker({ TSCHECKER: "tsc-rs-wasm" })).toBe("tsc-rs-wasm");
  });

  it("throws on an unknown value", () => {
    expect(() => parseTSChecker({ TSCHECKER: "tsc-rs" })).toThrowError(/TSCHECKER/);
  });
});

describe("parseTSCheckerWasmRecycleAfter", () => {
  it("returns undefined when unset or empty (shim default applies)", () => {
    expect(parseTSCheckerWasmRecycleAfter(undefined)).toBeUndefined();
    expect(parseTSCheckerWasmRecycleAfter({})).toBeUndefined();
    expect(parseTSCheckerWasmRecycleAfter({ TSCHECKER_WASM_RECYCLE_AFTER: "" })).toBeUndefined();
  });

  it("parses a positive integer override", () => {
    expect(parseTSCheckerWasmRecycleAfter({ TSCHECKER_WASM_RECYCLE_AFTER: "2" })).toBe(2);
    expect(parseTSCheckerWasmRecycleAfter({ TSCHECKER_WASM_RECYCLE_AFTER: "50" })).toBe(50);
    expect(parseTSCheckerWasmRecycleAfter({ TSCHECKER_WASM_RECYCLE_AFTER: "1" })).toBe(1);
  });

  it("throws honestly on non-positive or non-integer values", () => {
    expect(() =>
      parseTSCheckerWasmRecycleAfter({ TSCHECKER_WASM_RECYCLE_AFTER: "0" }),
    ).toThrowError(/TSCHECKER_WASM_RECYCLE_AFTER/);
    expect(() =>
      parseTSCheckerWasmRecycleAfter({ TSCHECKER_WASM_RECYCLE_AFTER: "-1" }),
    ).toThrowError(/TSCHECKER_WASM_RECYCLE_AFTER/);
    expect(() =>
      parseTSCheckerWasmRecycleAfter({ TSCHECKER_WASM_RECYCLE_AFTER: "abc" }),
    ).toThrowError(/TSCHECKER_WASM_RECYCLE_AFTER/);
    expect(() =>
      parseTSCheckerWasmRecycleAfter({ TSCHECKER_WASM_RECYCLE_AFTER: "2.5" }),
    ).toThrowError(/TSCHECKER_WASM_RECYCLE_AFTER/);
  });
});
