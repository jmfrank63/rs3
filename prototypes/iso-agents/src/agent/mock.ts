// The demo heart (CONTEXT / LLM-02): deterministic stateful scenarios keyed
// by a sniff of the conversation text (= the (task, spec) seed — the loop
// always includes task+spec in message 2, and feedback echoes the same
// words, so selection is stable across a run's iterations). Iteration 1
// plants a type error; iteration 2 fixes ONLY the type (a planted logic bug
// then fails the sandbox); iteration 3 is correct — every mock run proves
// the full gate sequence typecheck-fail -> sandbox-fail -> success. The
// mock always emits its OWN scenario code regardless of the requested
// function name; the typecheck/sandbox gates still operate genuinely on it.
// Output is ALWAYS clean JSON: fence stripping and brace salvage exist for
// real providers, not for this one (CONTEXT: malformed-retry path NOT in the
// v1 mock).
import type { LlmCompletion, LlmMessage, LlmProvider } from "./provider";

export const MOCK_PROVIDER_ID = "mock";

interface MockScenario {
  turns: string[];
}

function turn(explanation: string, code: string): string {
  return JSON.stringify({ explanation, files: { "guest.ts": code } });
}

// Planted progression (research, EXACT): iter1 TS2322 at `const slug: number`
// (+ a second TS2322 at `return slug`); iter2 type-fixed but drops
// trim/edge-strip so test 2 fails `expected "a-b", got "-a-b-"`; iter3
// correct. Iteration code stays type-legal in iters 2-3 under `strict`.
const SLUGIFY: MockScenario = {
  turns: [
    turn(
      "First draft of slugify (a type slip slipped in).",
      `function slugify(input: string): string {
  const slug: number = input.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return slug;
}
test("lowercases and hyphenates", () => { expect(slugify("Hello World")).toEqual("hello-world"); });
test("trims and collapses separators", () => { expect(slugify("  A -- B  ")).toEqual("a-b"); });
`,
    ),
    turn(
      "Fixed the declared type.",
      `function slugify(input: string): string {
  const slug: string = input.toLowerCase().replace(/[^a-z0-9]+/g, "-");
  return slug;
}
test("lowercases and hyphenates", () => { expect(slugify("Hello World")).toEqual("hello-world"); });
test("trims and collapses separators", () => { expect(slugify("  A -- B  ")).toEqual("a-b"); });
`,
    ),
    turn(
      "Now handles trim and edge separators too.",
      `function slugify(input: string): string {
  return input.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}
test("lowercases and hyphenates", () => { expect(slugify("Hello World")).toEqual("hello-world"); });
test("trims and collapses separators", () => { expect(slugify("  A -- B  ")).toEqual("a-b"); });
`,
    ),
  ],
};

// Planted progression (research, EXACT): iter1 TS2322 at `const next: string`;
// iter2 type-fixed but loop bound is `i < n` (off-by-one: fib(7) -> 8, the
// fib(0)/fib(1) test still passes — partial-pass feedback demo); iter3
// correct.
const FIBONACCI: MockScenario = {
  turns: [
    turn(
      "First draft of fibonacci (a type slip slipped in).",
      `function fibonacci(n: number): number {
  if (n <= 1) return n;
  let a = 0;
  let b = 1;
  for (let i = 2; i <= n; i++) {
    const next: string = String(a + b);
    a = b;
    b = next;
  }
  return b;
}
test("fib(0) and fib(1)", () => { expect(fibonacci(0)).toEqual(0); expect(fibonacci(1)).toEqual(1); });
test("fib(7) is 13", () => { expect(fibonacci(7)).toEqual(13); });
`,
    ),
    turn(
      "Fixed the declared type.",
      `function fibonacci(n: number): number {
  if (n <= 1) return n;
  let a = 0;
  let b = 1;
  for (let i = 2; i < n; i++) {
    const next = a + b;
    a = b;
    b = next;
  }
  return b;
}
test("fib(0) and fib(1)", () => { expect(fibonacci(0)).toEqual(0); expect(fibonacci(1)).toEqual(1); });
test("fib(7) is 13", () => { expect(fibonacci(7)).toEqual(13); });
`,
    ),
    turn(
      "Corrected the loop bound.",
      `function fibonacci(n: number): number {
  if (n <= 1) return n;
  let a = 0;
  let b = 1;
  for (let i = 2; i <= n; i++) {
    const next = a + b;
    a = b;
    b = next;
  }
  return b;
}
test("fib(0) and fib(1)", () => { expect(fibonacci(0)).toEqual(0); expect(fibonacci(1)).toEqual(1); });
test("fib(7) is 13", () => { expect(fibonacci(7)).toEqual(13); });
`,
    ),
  ],
};

// slug -> slugify scenario; anything else -> fibonacci, which is also the
// generic converging fallback for unknown tasks (CONTEXT).
function selectScenario(seed: string): MockScenario {
  return seed.includes("slug") ? SLUGIFY : FIBONACCI;
}

// One instance PER RUN (the route constructs providers per request), so the
// call counter is the run's iteration state — "stateful" across a run's
// iterations, never across runs. Calls past turn 3 keep returning turn 3
// (unreachable at default caps: the loop exits success at iteration 3).
export class MockLlmProvider implements LlmProvider {
  readonly id = MOCK_PROVIDER_ID;
  private calls = 0;

  complete(messages: LlmMessage[]): Promise<LlmCompletion> {
    const seed = messages.map((m) => m.content).join("\n").toLowerCase();
    const scenario = selectScenario(seed);
    const text = scenario.turns[Math.min(this.calls, scenario.turns.length - 1)];
    this.calls += 1;
    return Promise.resolve({ text });
  }
}
