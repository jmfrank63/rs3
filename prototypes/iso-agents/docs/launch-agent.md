# Launching an agent in the isolate

The worker IS the isolate. `wrangler dev` boots real workerd locally (no Cloudflare
account); every capability below is one stateless HTTP call, so any agent harness
(T3 Code, Claude Code, a script) drives it the same way.

## 0. Prerequisites + start

```sh
npm install          # pins: wrangler 4.149.0, typescript 6.0.3, quickjs + ts_rust.wasm vendored
npm run dev          # http://localhost:8787 — real workerd
```

Check it's alive:

```sh
curl -s localhost:8787/healthz | jq
# {ok:true, data:{status:"ok", runtime:{typechecker:"typescript@6.0.3 (in-isolate)"
#  | "ts-rust@multi-run/5ada636 (wasm)", sandbox:"quickjs-wasmfile-release-sync@0.32",
#  agent:"mock-provider ready", ...}}}
```

## 1. Launch an autonomous agent run (the main event)

```sh
curl -s -X POST localhost:8787/agent/run \
  -H 'content-type: application/json' \
  -d '{
    "task": "implement slugify",
    "spec": "slugify(s: string): string - lowercase, trim, collapse spaces/underscores to single hyphens, strip non-alphanumerics except hyphens, trim hyphens. Two test() calls.",
    "maxIterations": 5
  }' | jq '.data | {id, status, iterations: (.iterations | length), timings}'
```

What happens inside the isolate per iteration: LLM turn → guest.ts written to an in-memory
VFS → type-check (errors gate execution and feed back) → transpile → QuickJS-WASM sandbox
(deadline + memory limits, fresh context per run) → test() assertions → failures feed the
next LLM turn. Terminal statuses: `success | max_iterations | run_deadline | llm_error |
llm_malformed | internal_error`.

Fetch the full transcript later (in-memory store, isolate lifetime):

```sh
curl -s localhost:8787/agent/run/<id> | jq '.data.status, .data.timings'
```

## 2. Drive the isolate as a tool surface ("bash for agents")

For harnesses that want step-by-step control instead of the autonomous loop:

```sh
curl -s -X POST localhost:8787/agent/shell \
  -H 'content-type: application/json' \
  -d '{"commands":[
    {"op":"write","args":{"path":"/guest.ts","content":"..."}},
    {"op":"typecheck"},
    {"op":"run"},
    {"op":"cat","args":{"path":"/guest.ts"}},
    {"op":"ls"},{"op":"rm","args":{"path":"/guest.ts"}}
  ]}' | jq '.data.results[] | {op, ok, ms}'
```

Ops: `ls | cat | write | rm | typecheck | run` against a per-request VFS; `run` = transpile +
QuickJS execution with all guardrails (deadline, memory, no host eval).

## 3. Raw tools

- `POST /typecheck` — `{files:{"/t.ts":"..."}, runs?:N}` → diagnostics + N-run latency stats
  (the tsc-rs benchmark hook). In wasm mode add `"held":true` stats.
- `POST /sandbox/execute` — `{code}` → `{status: ok|error|timeout|crash, logs, tests, result, ms}`.

## 4. Real LLM instead of the mock

Zero keys → deterministic mock (converges in 3 iterations by design: type-fail → test-fail →
green). To use a real model:

```sh
cp .dev.vars.example .dev.vars   # then set:
#  LLM_PROVIDER=openai
#  LLM_BASE_URL=https://api.openai.com/v1   (or any OpenAI-compatible endpoint)
#  LLM_API_KEY=sk-...
#  LLM_MODEL=gpt-...                        (LLM_TIMEOUT_MS default 20000)
npm run dev   # healthz agent key flips to "openai-compatible"
```

## 5. Rust compiler mode (the fork)

```sh
npm run dev -- --var TSCHECKER:tsc-rs-wasm
# healthz typechecker -> "ts-rust@multi-run/5ada636 (wasm)"
# held instance: warm checks ~75-120ms (cold ~500ms); recycles after 50 runs
#   (TSCHECKER_WASM_RECYCLE_AFTER env) or on panic; /typecheck response gains held stats
```

## 6. Verify everything

```sh
npm run check && npm test          # 109 unit tests
npm run test:live                  # 5 live harnesses against real workerd (~250 assertions)
npm run bench:tsc-rs               # native tsc-rs vs in-isolate JS vs in-isolate WASM table
```

## T3 Code integration sketch

Point a harness loop at the isolate: send the task spec as one `/agent/run` call and treat
`.data.iterations` as the agent's transcript; or implement the harness's "run tests" tool as
`/agent/shell` with `write`+`typecheck`+`run`. Both are stateless HTTP — scale by
starting isolates, not by managing sandboxes.
