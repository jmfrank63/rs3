// Contract extraction (CONTEXT): the LLM must answer ONLY
// {"explanation": string, "files": Record<name, content>}. Real providers
// wrap that in markdown fences; strip them, then JSON.parse, then validate
// shape. Malformed -> MalformedContractError (the LOOP retries once with the
// corrective message, then terminates llm_malformed). import/export in any
// file -> ScriptViolationError (guests are script-style: transpileModule
// keeps `export`, which is a SyntaxError under QuickJS strict global eval —
// research-verified). "Malformed -> null" from the planning brief is
// realized as this typed error: a malformed answer NEVER yields a partial
// contract.
export interface LlmContract {
  explanation: string;
  files: Record<string, string>;
}

export class MalformedContractError extends Error {}

export class ScriptViolationError extends Error {
  constructor(readonly token: string) {
    super(`guest files must be script-style: source contains "${token}" (no import/export statements)`);
  }
}

// Same conservative trade as the sandbox's async gate: a word-boundary match
// in a comment is rejected too.
const SCRIPT_VIOLATION_PATTERN = /\b(?:import|export)\b/;

export function findScriptViolation(source: string): string | null {
  const match = SCRIPT_VIOLATION_PATTERN.exec(source);
  return match === null ? null : match[0];
}

export function extractContract(text: string): LlmContract {
  const stripped = stripFences(text);
  let parsed: unknown;
  try {
    parsed = JSON.parse(stripped);
  } catch {
    parsed = trySalvage(stripped);
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new MalformedContractError("contract is not a JSON object");
  }
  const obj = parsed as { explanation?: unknown; files?: unknown };
  if (obj.files === null || typeof obj.files !== "object" || Array.isArray(obj.files)) {
    throw new MalformedContractError("contract.files must be an object of {name: content}");
  }
  const files: Record<string, string> = {};
  for (const [name, content] of Object.entries(obj.files)) {
    if (typeof content !== "string") {
      throw new MalformedContractError(`contract.files.${name} must be a string`);
    }
    files[name] = content;
  }
  if (Object.keys(files).length === 0) {
    throw new MalformedContractError("contract.files must contain at least one file");
  }
  for (const [name, source] of Object.entries(files)) {
    const token = findScriptViolation(source);
    if (token !== null) {
      throw new ScriptViolationError(token);
    }
  }
  return { explanation: typeof obj.explanation === "string" ? obj.explanation : "", files };
}

// ```json ... ``` / ``` ... ``` — only a LEADING fence is stripped here; a
// missing trailing fence still parses via the brace-salvage path.
function stripFences(text: string): string {
  const trimmed = text.trim();
  const open = trimmed.match(/^```[a-zA-Z]*\s*\n/);
  if (open === null) {
    return trimmed;
  }
  return trimmed.slice(open[0].length).replace(/```\s*$/, "").trim();
}

// Real LLMs occasionally wrap the JSON in prose ("Here is the fix: {...}") —
// retry the parse on the outermost brace span.
function trySalvage(text: string): unknown {
  const first = text.indexOf("{");
  const last = text.lastIndexOf("}");
  if (first === -1 || last <= first) {
    throw new MalformedContractError("response contains no JSON object");
  }
  try {
    return JSON.parse(text.slice(first, last + 1));
  } catch {
    throw new MalformedContractError("response is not valid JSON (fenced, bare, or brace-salvaged)");
  }
}
