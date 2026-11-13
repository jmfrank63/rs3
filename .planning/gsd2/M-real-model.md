# Milestone: A real model whose thinking survives node loss

Read `AGENTS.md` and `docs/distributed-design.md` first. Steps 1 to 5 of the build order are done, all with the
deterministic `FakeModel` in `src/model.rs`. This milestone puts a real language model behind that seam and proves
the central claim of the design with it: every token is committed before anyone sees it, and a generation cut off
on one node continues on another without losing or repeating a token, and without sampling a committed position
again.

## Goal

A goal can run a real model, Qwen3-0.6B, on the CPU, through candle (pure Rust). Its output is real text, streamed
to T3 Code through the existing ACP adapter, with the model's thinking shown as thinking. When the node running it
dies, another node rebuilds the model's state from the committed prompt and tokens and goes on, so the final text is
byte-identical to an uninterrupted run. The fake model stays the default and every existing test keeps using it.

## Facts to build on

- This machine is a Ryzen 7 5700U (8 cores, AVX2, 61 GB) with no NVIDIA GPU and no CUDA. The GitHub runner is a
  4-core x86_64 VM. Everything in this milestone runs on the CPU.
- `candle-transformers` 0.11.1 has `models::quantized_qwen3`: `ModelWeights::from_gguf`, `forward(input, offset)`
  returning the last position's logits, and `clear_kv_cache()`. Use `candle-core`/`candle-transformers` 0.11.1 and
  the `tokenizers` crate unless research finds a blocker; do not add llama.cpp or other C++ bindings.
- The pinned model files, Apache-2.0:
  - `https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/23749fefcc72300e3a2ad315e1317431b06b590a/Qwen3-0.6B-Q8_0.gguf`,
    639,446,688 bytes, sha256 `9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031`.
  - `https://huggingface.co/Qwen/Qwen3-0.6B/resolve/c1899de289a04d12100db370d81485cdf75e47ca/tokenizer.json`,
    11,422,654 bytes, sha256 `aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4`.
  - Both are already in `~/.cache/rs3/models/` on this machine.
- `Record::InferenceStart` already records `engine`, `model_hash`, `tokenizer_hash`, `sampler`, `rng_state` and
  `prompt_tokens`. `Goals::generated_tokens()` counts every token a node samples, including any sampled again; it
  is the oracle for "no committed position is sampled twice".

## Required behaviour

- **Configuration.** `MODEL.PATH` (a GGUF file) and `MODEL.TOKENIZER` (a `tokenizer.json`) select the real model;
  with neither set the node uses the fake model, exactly as now. At start the node hashes both files with blake3,
  and new goals record `engine: "candle"`, those hashes and the prompt's real token ids.
- **Same model or hands off.** A node runs a goal only if its own model and tokenizer hashes equal the goal's. It
  never commits a token to a goal it cannot continue exactly; such a goal waits for a node that can.
- **Deterministic sampling.** Temperature and top-p are named constants. The random choice for a position derives
  only from the goal's seed and that position, so sampling needs no hidden state, and the same goal on any node
  gives the same tokens. Inference uses a fixed thread count (a setting with a constant default), and determinism
  must hold at that count.
- **Commit before show.** Unchanged: tokens are committed in batches before they are visible, exactly as with the
  fake model. The fake model's pacing (`token_secs`) does not slow the real model.
- **Restore, do not regenerate.** A worker that picks up a goal with committed tokens rebuilds the model state from
  `prompt_tokens` and the committed tokens without sampling any committed position, then samples the next one. The
  rebuild must take the same numerical path as the original run where it matters (for example, feed committed
  tokens the way decoding fed them), so the continuation is byte-identical to an uninterrupted run. If that proves
  impossible on candle's CPU path, report a blocker rather than weaken the test.
- **Ending.** The end-of-turn token ends the goal with reason `eos`, which ACP answers as `end_turn`. Reaching the
  budget ends it with reason `budget`, which ACP now answers as `max_tokens`. Output is valid UTF-8 at every
  committed batch boundary as seen through `GET /goals/{id}` and ACP (never split a character).
- **Conversation history.** A session turn's prompt is the model's chat template applied to every earlier turn of
  the session (its prompt and its final text) and then this turn's prompt. It is fixed when the turn is committed,
  so replays and restarts see the same prompt. The fake model ignores the template, so existing session tests keep
  their meaning.
- **Thinking.** Text the model writes between `<think>` and `</think>` streams to ACP as `agent_thought_chunk`
  updates and the rest as `agent_message_chunk`; a split tag across batches must still route correctly.
  `GET /goals/{id}` keeps the whole text. `session/load` replays both kinds.
- **Test model files.** A test helper downloads the two pinned files once into `RS3_MODEL_DIR` (default
  `~/.cache/rs3/models`), checks their sha256, and reuses them afterwards. A missing or wrong file fails the test with
  a clear message; it never skips. This lets CI run the real-model tests without any change to `ci.yml`.
- **Cost.** Real-model tests use short budgets (a few dozen tokens) and few nodes where the property allows, so
  `cargo test` and the CI job stay well inside its 45-minute limit, and mutation runs stay practical with `--file`.

## Acceptance tests

All are behavioural, start real `rs3` processes and bound every wait with a deadline.

1. **Real text.** One node with the real model answers "What is the capital of France? Answer with one word.
   /no_think" with text containing `Paris`, ending with reason `eos`.
2. **Deterministic.** The same goal with the same seed on two fresh single nodes gives byte-identical output.
3. **Restore after a crash.** `kill -9` a single node mid-goal and restart it. The goal finishes byte-identical to
   an uninterrupted run, and the tokens sampled across both runs equal the goal's token count.
4. **Failover.** On 5 nodes, kill the goal's owner and one other node mid-goal. Another node finishes it
   byte-identical to an uninterrupted run, no committed position is sampled again on any node, and once sealed the
   goal still reads back the same with two nodes down.
5. **Model mismatch.** In a cluster where one node has a different model file, that node commits no token to a goal
   created with the pinned model, and the goal still finishes on the others.
6. **Stop reasons.** A goal that hits its budget answers ACP `max_tokens`; one that ends answers `end_turn`.
7. **T3 turn with history.** Over ACP, turn 1 says "My name is Ada." and turn 2 asks "What is my name? /no_think";
   turn 2's text contains `Ada`. A turn with thinking streams `agent_thought_chunk` before the answer, and
   `session/load` from a new adapter replays both kinds byte-identically.
8. **Fake model unchanged.** With no `MODEL.*` settings, every existing test passes unchanged.

`scripts/ci-smoke.sh` also runs one real-model goal on a 3-node release cluster, killing the leader mid-goal, and
checks the output against a single-node run. Document the `MODEL.*` settings and how to fetch the files in
`Readme.md`, and add the model step to `docs/distributed-design.md`.

## Out of scope

GPU and CUDA builds (a later step on the RTX 3060 Ti machine; keep the device choice behind the model seam),
snapshots of the KV cache, other models or model downloads in production, tool calls, the iso-agents coding loop in
`prototypes/iso-agents` (the next milestone), changes to `ci.yml`, growing membership, libvirt or Docker nodes,
power-loss tests.

Prefer the shortest design that satisfies these tests. Split it into small slices, each with a demonstrable
behaviour. Start with real text and determinism on one node, then restore after a crash, then the model check and
failover in a cluster, then stop reasons, history and thinking over ACP, then the smoke test and docs.
