// Pure console/serialization policy for the sandbox (v5 H9/H10): ZERO
// imports, so vitest specs exercise it without dragging the quickjs graph
// into the worker transport (Phase 2 lesson). The runtime binds these to
// real guest contexts; the policy itself is plain host-side data logic.

// Console buffer soft cap in RETAINED bytes (03-RESEARCH console capture).
// v5 H10: the cap bounds what is KEPT, not what the guest may print —
// oversized entries are truncated/marked at retention time, never stored
// in full first.
export const MAX_LOG_BYTES = 8192;

// Host-side JSON-safe projection (H9): context.dump() yields plain host
// data, but guest BigInts arrive as host BigInts and self-referential
// structures as cyclic objects — either used to throw inside
// JSON.stringify and 500 the HTTP layer (result) or killed the console
// capture (guest-visible serialization error). Policy: BigInt -> decimal
// string, cycles -> "[circular]", everything else passes through. Shared
// (acyclic) references survive — only true cycles are cut.
export function jsonSafe(value: unknown, seen?: WeakSet<object>): unknown {
  if (typeof value === "bigint") {
    return value.toString();
  }
  if (value === null || typeof value !== "object") {
    return value;
  }
  const visited = seen ?? new WeakSet<object>();
  if (visited.has(value)) {
    return "[circular]";
  }
  visited.add(value);
  const safe: unknown = Array.isArray(value)
    ? value.map((element) => jsonSafe(element, visited))
    : Object.fromEntries(
        Object.entries(value).map(([key, element]) => [key, jsonSafe(element, visited)] as const),
      );
  visited.delete(value);
  return safe;
}

function argBytes(value: unknown): number {
  return (typeof value === "string" ? value.length : JSON.stringify(value)?.length ?? 8) + 1;
}

// Per-entry retention budget (H10): returns the args worth keeping and
// their accounted byte cost, both guaranteed within `budget`. Strings are
// truncated to the remaining capacity; oversized non-strings collapse to a
// bounded marker (never a full dump). A 1 MB console string therefore
// retains at most `budget` bytes.
export function fitLogArgs(
  args: unknown[],
  budget: number,
): { args: unknown[]; bytes: number } {
  const kept: unknown[] = [];
  let bytes = 0;
  for (const value of args) {
    const remaining = budget - bytes;
    if (remaining <= 0) {
      break;
    }
    if (typeof value === "string") {
      const slice = value.slice(0, remaining - 1);
      kept.push(slice);
      bytes += slice.length + 1;
    } else if (argBytes(value) <= remaining) {
      kept.push(value);
      bytes += argBytes(value);
    } else {
      kept.push("[unserializable]");
      bytes += 16; // "[unserializable]" + separator
    }
  }
  return { args: kept, bytes };
}
