# Milestone: T3 Code drives rs3 through ACP

Read `AGENTS.md` and `docs/distributed-design.md` first. This is step 5 of the build order. Steps 1 to 4 are done:
a 3 to 5 node Raft cluster runs durable goals, keeps every committed token through the loss of 2 nodes, and seals
ended goals with Reed-Solomon.

## Goal

A T3 Code thread can talk to the rs3 cluster as if it were one agent. T3 Code is the control surface; rs3 owns agent
state and failover. T3 Code already runs any local command that speaks the Agent Client Protocol (ACP) on stdio
("Local ACP command" in its provider settings), so rs3 needs no change to T3 Code: `rs3 acp <node-url>...` is an
ACP agent. It holds no state of its own, so it can be killed at any moment. When nodes die under a running turn, the
thread keeps streaming with no gap, no repeat and no token generated twice.

## Protocol

ACP version 1: newline-delimited JSON-RPC 2.0 on stdin and stdout. Stdout carries protocol messages only; logs go to
stderr. Implement only what is listed here. Read the schema at https://agentclientprotocol.com/protocol/schema, and
prefer hand-written `serde_json` types over a new dependency unless research finds a good reason.

- `initialize` answers `protocolVersion: 1`, `agentCapabilities: {loadSession: true}`, `authMethods: []`.
- `session/new` (`cwd`, `mcpServers`) answers `{sessionId}`.
- `session/prompt` (`sessionId`, `prompt`: content blocks) streams `session/update` notifications with
  `update: {sessionUpdate: "agent_message_chunk", content: {type: "text", text}}`, then answers `{stopReason}`.
- `session/load` (`sessionId`, `cwd`, `mcpServers`) replays the conversation as `user_message_chunk` and
  `agent_message_chunk` updates, then answers `null`.
- `session/cancel` (notification) cancels the session's running turn.

## Required behaviour

- **One goal per prompt.** Each prompt creates exactly one goal, even when the adapter must send the request again
  because the first attempt ended in `outcome_unknown` or a connection loss. The cluster deduplicates on a key made
  of the session and its turn number, committed through Raft. Plain `POST /goals` keeps working unchanged.
- **Sessions live in the cluster.** Session id, turns, their prompts and goal ids are committed through Raft, so any
  node can answer for them, and they survive the same failures as goals. A session id is unique across adapters
  and restarts.
- **Streaming with failover.** The adapter follows the goal's committed output and sends only new text. When a node
  stops answering, it moves to the next URL and continues from its offset. Text sent to the client is byte-identical
  to the goal's final output. Use what exists (polling `GET /goals/{id}` is fine) unless a test shows it is too slow.
- **Stop reasons.** `done` answers `end_turn`, `cancelled` answers `cancelled`. `failed` answers a JSON-RPC error
  carrying the goal's error code, and the text already streamed stays.
- **Cancel.** `session/cancel` cancels the goal durably, and the pending `session/prompt` answers `cancelled`. It
  must be handled while a prompt is in flight.
- **Parallel sessions.** One adapter process serves several sessions at once. Each streams independently and neither
  blocks the other. A second prompt on a session whose turn is in flight in this process answers a JSON-RPC error.
- **No quorum is not failure.** While the cluster answers `no_quorum` or no node answers, the turn waits and creates
  nothing new. It resumes when quorum returns. After a named timeout (`ACP_UNAVAILABLE_TIMEOUT`, default 300 s) the
  prompt answers a JSON-RPC error `cluster_unavailable`; the goal is untouched and nothing is lost.
- **Adapter restart.** After the adapter is killed, `session/load` from a new adapter replays every committed turn,
  sealed ones included, through any live node, and answers without waiting for a running turn to end. The next
  `session/prompt` on that session first streams the rest of the unfinished turn, then runs its own prompt. T3 Code
  ignores updates sent outside a turn, so the adapter must not send them.
- **Scripts.** The agent advertises one slash command, `run`, through an `available_commands_update` after
  `session/new` and `session/load`. A prompt `/run <key>` starts a script goal for the stored script under `<key>` as
  one turn, with the same exactly-once and streaming rules. An unknown key answers a JSON-RPC error.
- **Robust input.** A malformed line answers `-32700`, an unknown method `-32601`, an unknown session `-32602`, and
  none of them end the process. Stdin closing ends the adapter cleanly and leaves its goals running.
- **Generation settings.** Prompts carry no budget or seed. The budget is a named constant or setting, and the seed
  is derived from the session and turn, so a replay is deterministic. The fake model stays: each turn's goal sees
  only that turn's prompt, and passing conversation history to a model belongs to the real-model milestone.

## Acceptance tests

All are behavioural. They start real `rs3` processes and a real `rs3 acp` child, speak ACP over its pipes, and bound
every wait with a deadline:

1. **Turn:** `initialize`, `session/new`, `session/prompt` on one node. Concatenated chunks equal
   `GET /goals/{id}` output, the stop reason is `end_turn`, and stdout held nothing but valid JSON-RPC.
2. **Failover:** on 5 nodes, during a turn, `kill -9` two nodes including the owner and the node the adapter is
   talking to. The stream continues and the concatenated text is byte-identical to the same turn on a single node.
3. **Exactly one goal:** a request whose response is lost (for example the node killed right after the commit, or a
   test proxy dropping the response) still yields one goal and one turn, verified on the cluster.
4. **Quorum loss:** three of five nodes killed mid-turn. The prompt stays pending and no goal is added; when one node
   returns, the turn completes byte-identical. A variant with a short timeout answers `cluster_unavailable`, and a
   later `session/load` still shows the committed text.
5. **Cancel:** `session/cancel` mid-turn answers `cancelled`, and the goal is `cancelled` on every node.
6. **Parallel:** two sessions in one adapter stream at the same time, with overlapping chunks, and each ends
   byte-identical to its own goal.
7. **Restart:** kill the adapter mid-turn. A new adapter's `session/load` replays the conversation, the next prompt
   delivers the rest of the unfinished turn with no gap or repeat, then its own turn. No extra goal exists for the
   old turn.
8. **Sealed replay:** `session/load` of a session whose goals are sealed, with two nodes killed, replays byte-identical.
9. **Script:** `/run <key>` is advertised and runs a stored script once; its writes apply once across a failover.
10. **Bad input:** malformed JSON, an unknown method and an unknown session each get their error code and the same
    process then completes a normal turn.

`scripts/ci-smoke.sh` also runs one ACP turn against a 3-node release cluster, killing the leader during the turn.

Document in `Readme.md` how to add rs3 under T3 Code's "Local ACP command", and mark step 5 in
`docs/distributed-design.md`.

## Out of scope

Patching T3 Code, ACP v2, authentication and TLS, permission requests, tool calls and file edits in the T3 workspace,
MCP servers, `session/list`, plan and mode switching, model selection, real models, growing membership, Docker and
libvirt nodes, power-loss tests.

Prefer the shortest design that satisfies these tests. Split it into small slices, each with a demonstrable
behaviour. Start with a single-node turn over ACP, then durable sessions and exactly-once prompts, then failover and
quorum loss, then cancel and parallel sessions, then restart and sealed replay, then scripts and the smoke test.
