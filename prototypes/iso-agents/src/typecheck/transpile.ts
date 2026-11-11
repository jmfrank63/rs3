// Script-safe transpile for the sandbox (research-verified against
// typescript@6.0.3): transpileModule reports SYNTACTIC diagnostics only —
// never semantic errors, so the typecheck gate MUST run first — and
// `export` survives under module-aware settings, which would SyntaxError in
// QuickJS strict global eval. Guests are therefore gate-enforced
// script-style (extractor) before this runs. Conservative options mirror
// TranspileOnlyTypechecker: target ES2022 + isolatedModules, no `module`
// override (guests never import, so the module kind is inert).
import ts from "typescript";
import { mapDiagnostic, type TypecheckDiagnostic } from "./index";

export interface TranspileResult {
  js: string;
  diagnostics: TypecheckDiagnostic[];
  ms: number;
}

export function transpileGuest(source: string): TranspileResult {
  const t0 = performance.now();
  const r = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, isolatedModules: true },
    fileName: "guest.ts",
    reportDiagnostics: true,
  });
  return {
    js: r.outputText,
    diagnostics: (r.diagnostics ?? []).map(mapDiagnostic),
    ms: performance.now() - t0,
  };
}
