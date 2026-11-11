import { describe, expect, it } from "vitest";
import { extractContract, MalformedContractError, ScriptViolationError } from "../src/agent/extractor";
import { MockLlmProvider } from "../src/agent/mock";
import type { LlmMessage } from "../src/agent/provider";

// Pure agent pieces ONLY (small-module discipline): extractor + mock. The
// loop's seams enter as stubs in loop.spec.ts; real typecheck/sandbox/LLM
// integration is proven by scripts/test-agent-live.mjs (04-02).

const seedMessages = (seed: string): LlmMessage[] => [{ role: "user", content: seed }];

describe("contract extractor", () => {
  it("parses fenced and bare JSON into {explanation, files}", () => {
    const contract = { explanation: "did the thing", files: { "guest.ts": "const x = 1;\n" } };
    const fenced = "```json\n" + JSON.stringify(contract) + "\n```";
    expect(extractContract(fenced)).toEqual(contract);
    expect(extractContract(JSON.stringify(contract))).toEqual(contract);
  });

  it("rejects shape violations with MalformedContractError", () => {
    expect(() => extractContract('{"files": {}}')).toThrow(MalformedContractError);
    expect(() => extractContract('{"files": {"a.ts": 1}}')).toThrow(MalformedContractError);
    expect(() => extractContract('{"files": []}')).toThrow(MalformedContractError);
    expect(() => extractContract("[1,2,3]")).toThrow(MalformedContractError);
  });

  it("never returns a partial contract for garbage input", () => {
    expect(() => extractContract("nope")).toThrow(MalformedContractError);
    expect(() => extractContract("prose { then broken")).toThrow(MalformedContractError);
  });

  it("rejects import/export files with ScriptViolationError naming the token", () => {
    try {
      extractContract(JSON.stringify({ files: { "guest.ts": "import x from 'y';\n" } }));
      expect.unreachable();
    } catch (err) {
      expect(err).toBeInstanceOf(ScriptViolationError);
      expect((err as ScriptViolationError).message).toContain("import");
    }
    expect(() =>
      extractContract(JSON.stringify({ files: { "guest.ts": "export const a = 1;\n" } })),
    ).toThrow(ScriptViolationError);
  });
});

describe("mock provider scenarios", () => {
  it("slugify scenario converges over exactly 3 deterministic turns", async () => {
    const mock = new MockLlmProvider();
    const texts: string[] = [];
    for (let i = 0; i < 3; i++) {
      const completion = await mock.complete(seedMessages("implement slugify\n\nSPEC:\nslugify(input)"));
      texts.push(completion.text);
    }
    const turn1 = extractContract(texts[0]);
    const turn2 = extractContract(texts[1]);
    const turn3 = extractContract(texts[2]);
    expect(turn1.files["guest.ts"]).toContain("const slug: number");
    expect(turn2.files["guest.ts"]).toContain("const slug: string");
    expect(turn2.files["guest.ts"]).not.toContain("trim()");
    expect(turn3.files["guest.ts"]).toContain("trim()");
    const replay = new MockLlmProvider();
    const replayText = (await replay.complete(seedMessages("implement slugify\n\nSPEC:\nslugify(input)"))).text;
    expect(replayText).toBe(texts[0]);
  });

  it("unknown tasks get the fibonacci generic fallback, deterministically", async () => {
    const mock = new MockLlmProvider();
    const text1 = (await mock.complete(seedMessages("mystery task\n\nSPEC:\nsomething"))).text;
    const turn1 = extractContract(text1);
    expect(turn1.files["guest.ts"]).toContain("function fibonacci");
    expect(turn1.files["guest.ts"]).toContain(": string");
    const other = new MockLlmProvider();
    const text2 = (await other.complete(seedMessages("mystery task\n\nSPEC:\nsomething"))).text;
    expect(text2).toBe(text1);
  });
});
