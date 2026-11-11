import { describe, expect, it, vi, afterEach } from "vitest";
import { LlmError, OpenAiCompatibleLlmProvider } from "../src/agent/provider";
import type { LlmMessage } from "../src/agent/provider";

// v5 H4 regression: provider attempts inherit the RUN's remaining budget —
// no fresh timeout per attempt, retries only while budget remains, and an
// already-expired deadline never reaches the network. Fetch is stubbed at
// the global (workerd-safe: no dynamic codegen anywhere in this spec).

const messages: LlmMessage[] = [{ role: "user", content: "hi" }];

const okResponse = (): Response =>
  new Response(JSON.stringify({ choices: [{ message: { content: "ok" } }] }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("provider run-deadline budget (H4)", () => {
  it("refuses to start when the deadline is already spent (zero fetches)", async () => {
    let fetches = 0;
    vi.stubGlobal("fetch", (_url: string, _init?: RequestInit): Promise<Response> => {
      fetches += 1;
      return Promise.resolve(okResponse());
    });
    const provider = new OpenAiCompatibleLlmProvider("http://blackhole", "m", undefined, 20000);
    await expect(provider.complete(messages, { deadlineMs: performance.now() - 1 })).rejects.toThrow(
      /deadline exhausted before the request/,
    );
    expect(fetches).toBe(0);
  });

  it("skips the retry once the first attempt consumed the remaining budget", async () => {
    let fetches = 0;
    vi.stubGlobal("fetch", (_url: string, _init?: RequestInit): Promise<Response> => {
      fetches += 1;
      // Attempt 1 burns ~60ms of a ~30ms budget, then fails retryably
      // (network class). A second attempt must never start.
      return sleep(60).then(() => {
        throw new TypeError("blackholed");
      });
    });
    const provider = new OpenAiCompatibleLlmProvider("http://blackhole", "m", undefined, 20000);
    await expect(
      provider.complete(messages, { deadlineMs: performance.now() + 30 }),
    ).rejects.toThrow(/deadline exhausted before the retry/);
    expect(fetches).toBe(1);
  });

  it("still retries when budget remains after a retryable failure", async () => {
    let fetches = 0;
    vi.stubGlobal("fetch", (_url: string, _init?: RequestInit): Promise<Response> => {
      fetches += 1;
      if (fetches === 1) {
        return Promise.resolve(new Response("boom", { status: 502 }));
      }
      return Promise.resolve(okResponse());
    });
    const provider = new OpenAiCompatibleLlmProvider("http://up", "m", undefined, 20000);
    const completion = await provider.complete(messages, { deadlineMs: performance.now() + 20000 });
    expect(completion.text).toBe("ok");
    expect(fetches).toBe(2);
  });

  it("non-retryable failures never get a second attempt (budget untouched)", async () => {
    let fetches = 0;
    vi.stubGlobal("fetch", (_url: string, _init?: RequestInit): Promise<Response> => {
      fetches += 1;
      return Promise.resolve(new Response("nope", { status: 400 }));
    });
    const provider = new OpenAiCompatibleLlmProvider("http://up", "m", undefined, 20000);
    await expect(provider.complete(messages)).rejects.toThrow(LlmError);
    expect(fetches).toBe(1);
  });
});
