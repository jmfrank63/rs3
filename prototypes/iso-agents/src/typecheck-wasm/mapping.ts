// Pure mapping from ts_goport's JSON diagnostics shape to the repo's flat
// TypecheckDiagnostic. ZERO runtime imports (the only import is a type,
// erased at emit), so vitest specs exercise it without dragging the 5.1 MB
// wasm module OR the 8.8 MB typescript build into the worker transport
// (Phase 2 lesson).
import type { TypecheckDiagnostic } from "../typecheck/index";

// Wire shape (06-RESEARCH probe transcripts): file diagnostics carry
// startPosition {line, character} ZERO-based plus sourceLines/endPosition;
// config diagnostics carry pos/end = -1 and NO startPosition/fileName.
// `category` is tsc's numeric enum (0 Warning, 1 Error, 2 Message,
// 3 Suggestion).
export interface WasmSourcePosition {
  line: number;
  character: number;
}

export interface WasmDiagnostic {
  fileName?: string;
  startPosition?: WasmSourcePosition;
  code: number;
  category: number;
  text: string;
}

const CATEGORY_NAMES = ["Warning", "Error", "Message", "Suggestion"] as const;

// Guests are seeded under /app (the spike-proven tsconfig layout — see
// checker.ts); reported fileNames are stripped back to the repo's rooted
// form ("/app/guest.ts" -> "/guest.ts") so diagnostics are byte-identical
// across checker implementations.
function repoFileName(fileName: string): string {
  return fileName.startsWith("/app/") ? `/${fileName.slice("/app/".length)}` : fileName;
}

export function mapWasmDiagnostic(raw: WasmDiagnostic): TypecheckDiagnostic {
  const start = raw.startPosition;
  return {
    code: raw.code,
    message: raw.text,
    line: start ? start.line + 1 : -1,
    column: start ? start.character + 1 : -1,
    file: raw.fileName !== undefined ? repoFileName(raw.fileName) : "(program)",
    category: CATEGORY_NAMES[raw.category] ?? String(raw.category),
  };
}

// Same ordering semantics as the ts-js checker's byLocation (file, line,
// column) — duplicated here because importing it would drag the typescript
// runtime into this module's graph.
function byLocation(a: TypecheckDiagnostic, b: TypecheckDiagnostic): number {
  return a.file.localeCompare(b.file) || a.line - b.line || a.column - b.column;
}

export function mapWasmDiagnostics(raw: WasmDiagnostic[]): TypecheckDiagnostic[] {
  return raw.map(mapWasmDiagnostic).sort(byLocation);
}
