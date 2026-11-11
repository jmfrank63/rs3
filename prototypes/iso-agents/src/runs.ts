// In-memory run registry (Phase 1 seam), grown in Phase 4 (OBS-02): records
// carry the full transcript + timings; terminal statuses are loop.ts's
// AgentStatus composed with "running" (CONTEXT locked set — ROADMAP's
// deadline name is renamed "run_deadline" and "llm_malformed" is added; G1).
// FIFO cap at 50 runs (Map insertion-order eviction; transcripts are a few
// hundred KB worst case -> low MB, safely inside the 128 MB isolate — G8).
import type { AgentIteration, AgentStatus, AgentTimings } from "./agent/loop";

export type RunStatus = "running" | AgentStatus | "internal_error";

export interface RunRecord {
  id: string;
  status: RunStatus;
  task: string;
  createdAt: string;
  finishedAt: string | null;
  iterations: AgentIteration[];
  timings: AgentTimings | null;
}

export const MAX_STORED_RUNS = 50;

const runs = new Map<string, RunRecord>();

export const runRegistry = {
  create(task: string): RunRecord {
    if (runs.size >= MAX_STORED_RUNS) {
      // v5 H7 admission control: a RUNNING record belongs to an in-flight
      // request — evicting it used to orphan that request's finalize (the
      // registry lookup missed and the transcript stayed "running" forever).
      // Prefer the oldest TERMINATED record; only when the store is all
      // running (a >50-concurrent burst) fall back to plain FIFO so the
      // cap still bounds memory.
      let evicted = false;
      for (const [id, record] of runs) {
        if (record.status !== "running") {
          runs.delete(id);
          evicted = true;
          break;
        }
      }
      if (!evicted) {
        const oldest = runs.keys().next();
        if (oldest.done !== true) {
          runs.delete(oldest.value);
        }
      }
    }
    const run: RunRecord = {
      id: crypto.randomUUID(),
      status: "running",
      task,
      createdAt: new Date().toISOString(),
      finishedAt: null,
      iterations: [],
      timings: null,
    };
    runs.set(run.id, run);
    return run;
  },
  // Unchanged Phase 1 contract; the 404 route path rides on this.
  get(id: string): RunRecord | undefined {
    return runs.get(id);
  },
  // v5 H7: finalizes the CALLER-HELD record reference — the object is
  // mutated regardless of registry membership (create returns the stored
  // object, so the normal path still mutates in place and the POST handler
  // serializes it after finalize). A FIFO eviction while the run awaited
  // its provider can no longer leave the transcript stuck "running": the
  // evicted object still reaches its terminal status (its registry lookup
  // simply 404s from then on). finishedAt uses wall-clock ISO time
  // (transcript timing fidelity is performance.now()'s job; this is
  // registry bookkeeping).
  finalize(run: RunRecord, status: RunStatus, iterations: AgentIteration[], timings: AgentTimings | null): void {
    run.status = status;
    run.iterations = iterations;
    run.timings = timings;
    run.finishedAt = new Date().toISOString();
  },
};
