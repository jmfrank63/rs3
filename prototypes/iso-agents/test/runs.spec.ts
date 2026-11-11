import { MAX_STORED_RUNS, runRegistry } from "../src/runs";
import { describe, expect, it } from "vitest";

describe("runRegistry", () => {
  it("creates a record with uuid id, running status, task, and empty transcript", () => {
    const run = runRegistry.create("probe task");
    expect(typeof run.id).toBe("string");
    expect(run.id.length).toBe(36);
    expect(run.status).toBe("running");
    expect(run.task).toBe("probe task");
    expect(run.finishedAt).toBeNull();
    expect(run.iterations).toEqual([]);
    expect(run.timings).toBeNull();
    expect(() => new Date(run.createdAt).toISOString()).not.toThrow();
    expect(runRegistry.get(run.id)).toEqual(run);
  });

  it("returns undefined for an unknown id", () => {
    expect(runRegistry.get("nope")).toBeUndefined();
  });

  it("finalize stamps status, transcript, timings, and finishedAt", () => {
    const run = runRegistry.create("finalized task");
    const iterations = [
      {
        n: 1,
        llmMs: 1,
        explanation: "x",
        files: { "guest.ts": "1" },
        typecheck: null,
        transpileMs: null,
        sandbox: null,
        feedback: "fb",
      },
    ];
    const timings = { llmMs: 1, typecheckMs: 2, transpileMs: 3, sandboxMs: 4, totalMs: 5 };
    runRegistry.finalize(run, "max_iterations", iterations, timings);
    const stored = runRegistry.get(run.id);
    expect(stored?.status).toBe("max_iterations");
    expect(stored?.iterations).toEqual(iterations);
    expect(stored?.timings).toEqual(timings);
    expect(stored?.finishedAt).not.toBeNull();
  });

  it("H7: finalize stamps the caller-held record even after FIFO eviction", () => {
    // All-running store at the cap -> the fallback FIFO evicts the oldest
    // (still-running) victim. The request holds its reference: finalize
    // must terminate THAT object even though the registry forgot it —
    // no record left stuck "running".
    const victim = runRegistry.create("evicted while running");
    // cap + 2 inserts guarantees the victim (oldest of the all-running
    // store) is evicted by the fallback FIFO whatever preceded this test.
    for (let i = 0; i < MAX_STORED_RUNS + 2; i++) {
      runRegistry.create("filler");
    }
    expect(runRegistry.get(victim.id)).toBeUndefined(); // evicted
    runRegistry.finalize(victim, "run_deadline", [], null);
    expect(victim.status).toBe("run_deadline");
    expect(victim.finishedAt).not.toBeNull();
    expect(victim.timings).toBeNull();
    expect(runRegistry.get(victim.id)).toBeUndefined();
  });

  it("H7: create never evicts a running record while terminated ones exist", () => {
    const active = runRegistry.create("active in-flight run");
    for (let i = 0; i < MAX_STORED_RUNS - 1; i++) {
      runRegistry.finalize(runRegistry.create("done filler"), "max_iterations", [], null);
    }
    // Store is at the cap: [active(running), done, done, ...]. One more
    // create must evict the oldest TERMINATED record, never the active one.
    const fresh = runRegistry.create("the newest");
    expect(runRegistry.get(active.id)).toBeDefined();
    expect(active.status).toBe("running");
    expect(runRegistry.get(fresh.id)).toBeDefined();
    expect(runRegistry.get(fresh.id)).toBe(fresh);
  });

  it("caps the store at MAX_STORED_RUNS with FIFO eviction", () => {
    // Insert-count math keeps this independent of earlier tests in the file:
    // 51 fresh inserts always evict at least the first of them, whatever the
    // pre-existing size P (evictions = P + 2, oldest globally first).
    const first = runRegistry.create("will be evicted");
    for (let i = 0; i < MAX_STORED_RUNS; i++) {
      runRegistry.create("filler");
    }
    expect(runRegistry.get(first.id)).toBeUndefined();
    const fresh = runRegistry.create("the newest");
    expect(runRegistry.get(fresh.id)).toBeDefined();
  });
});
