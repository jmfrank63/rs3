// Guest file validation for the tsc-rs-wasm checker (v5 H1/H2): a PURE
// module — zero imports — so vitest specs exercise it without dragging the
// 5.1 MB wasm module into the worker transport (Phase 2 lesson). The guard
// runs twice by design: the HTTP layer (/typecheck) calls it for an early
// 400 bad_request, and the checker's seed calls it again so no entry point
// (agent loop, /agent/shell) can bypass it.

// Repo-rooted canonical form for a submitted path — identical rooting to
// the HTTP layer and the loop's rootFiles: bare names gain a leading "/".
export function canonicalGuestPath(name: string): string {
  return name.startsWith("/") ? name : `/${name}`;
}

// Only TypeScript sources may enter the check project. ".d.ts" ends in
// ".ts", so ambient declarations pass; everything else (tsconfig.json,
// .txt, extensionless) is rejected — a non-source file can only confuse
// the checker, never help the guest.
export function isGuestSourcePath(canonical: string): boolean {
  return canonical.endsWith(".ts");
}

export class GuestFileError extends Error {}

// H2: compiler-owned paths. The checker seeds /app/tsconfig.json (project
// config — a guest copy like {"noCheck":true} used to overwrite it and buy
// a clean verdict with zero checking) and /app/guest-globals.d.ts (trusted
// ambient declarations — a guest file of the same name used to be silently
// shadowed by the trusted seed and never checked). Canonical repo-rooted
// names: the checker roots every submitted path by exactly this function.
export const RESERVED_GUEST_PATHS: ReadonlySet<string> = new Set([
  "/tsconfig.json",
  "/guest-globals.d.ts",
]);

// H1+H2: throws GuestFileError on a non-.ts/.d.ts path or a compiler-owned
// name; returns the canonicalized name so callers seed exactly what was
// validated.
export function assertValidGuestFile(name: string): string {
  const canonical = canonicalGuestPath(name);
  if (!isGuestSourcePath(canonical)) {
    throw new GuestFileError(`guest file ${canonical} must be a .ts or .d.ts source file`);
  }
  if (RESERVED_GUEST_PATHS.has(canonical)) {
    throw new GuestFileError(`guest file ${canonical} collides with a compiler-owned path`);
  }
  return canonical;
}

// v6 calibration (live cliff-search against the shipped module): the
// checker's parser recurses per expression-nesting level and the wasm
// build's stack overflows somewhere between depth 600 (ok, 203 ms) and
// depth 1200 (uncatchable 500) — raw line count is irrelevant (4000 flat
// lines check fine in ~70 ms; a 3000-term `a+b+…` chain is what crashed
// the first time, at recursion depth ~3000). The limit sits 2.5x under
// the cliff and far above real-world code (typical max nesting < 50).
// Depth, not lines, is the guard metric.
export const WASM_GUEST_LIMITS = {
  maxNestingDepth: 256,
  maxSourceBytes: 262144,
} as const;

export class WasmGuestTooComplexError extends Error {}

// Longest stretch of nesting over () [] {} in `source`, ignoring brackets
// inside strings, template literals, and comments — with `${` holes in
// templates scanned as code (the `}` that closes a hole returns to the
// template). Contexts live on one stack: "(", "[", "{" and "`" (template).
// Known blind spot: regex literals are scanned as code, so unbalanced
// brackets inside a regex can skew the count in either direction — the
// runtime catch (recycle on throw) is the backstop for underestimates.
export function scanMaxNestingDepth(source: string): number {
  let max = 0;
  let depth = 0;
  const stack: string[] = [];
  let state: "code" | "line" | "block" | "single" | "double" | "template" = "code";
  for (let i = 0; i < source.length; i++) {
    const ch = source[i];
    const next = source[i + 1];
    if (state === "line") {
      if (ch === "\n") state = "code";
    } else if (state === "block") {
      if (ch === "*" && next === "/") {
        state = "code";
        i++;
      }
    } else if (state === "single" || state === "double") {
      if (ch === "\\") i++;
      else if ((state === "single" && ch === "'") || (state === "double" && ch === '"')) state = "code";
    } else if (state === "template") {
      if (ch === "\\") i++;
      else if (ch === "`") {
        stack.pop();
        state = "code";
      } else if (ch === "$" && next === "{") {
        stack.push("{");
        depth++;
        if (depth > max) max = depth;
        state = "code";
        i++;
      }
    } else {
      if (ch === "/" && next === "/") {
        state = "line";
        i++;
      } else if (ch === "/" && next === "*") {
        state = "block";
        i++;
      } else if (ch === "'") state = "single";
      else if (ch === '"') state = "double";
      else if (ch === "`") {
        stack.push("`");
        state = "template";
      } else if (ch === "(" || ch === "[" || ch === "{") {
        stack.push(ch);
        depth++;
        if (depth > max) max = depth;
      } else if (ch === ")" || ch === "]" || ch === "}") {
        const top = stack.pop();
        if (top && top !== "`") depth--;
        if (top === "{" && stack[stack.length - 1] === "`") {
          stack.pop();
          state = "template";
        }
      }
    }
  }
  return max;
}

// Throws WasmGuestTooComplexError when a guest exceeds the calibrated
// wasm-stack limits. The HTTP layer maps it to 400 bad_request with the
// ts-js fallback hint; the checker's seed calls it so no entry point
// bypasses it (the agent loop treats it as a guest error and feeds it
// back to the LLM).
export function assertGuestWithinWasmLimits(name: string, content: string): void {
  if (content.length > WASM_GUEST_LIMITS.maxSourceBytes) {
    throw new WasmGuestTooComplexError(
      `guest file ${name} is ${content.length} bytes; the wasm checker accepts up to ${WASM_GUEST_LIMITS.maxSourceBytes} (use TSCHECKER=ts-js for large sources)`,
    );
  }
  const depth = scanMaxNestingDepth(content);
  if (depth > WASM_GUEST_LIMITS.maxNestingDepth) {
    throw new WasmGuestTooComplexError(
      `guest file ${name} nests ${depth} deep; the wasm checker accepts up to ${WASM_GUEST_LIMITS.maxNestingDepth} (use TSCHECKER=ts-js for deeply nested sources)`,
    );
  }
}
