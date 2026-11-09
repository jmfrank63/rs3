# Distributed agent design

rs3 becomes a 3 to 5 node cluster that presents itself as one database and one agent, and keeps working when
2 of 5 nodes are lost. Nodes are simulated: Docker for daily failover tests, libvirt VMs for power-loss tests.

## Decisions

- **One agent, many goals.** The agent runs several goals in parallel. Each goal has a priority, a budget and a
  graph of tasks. All nodes do useful work, so there are no idle standbys. Scripts run near their data.
- **Consistency over availability.** In a partition the majority keeps working. The minority stops committing,
  and its leases expire. Raft orders the work. It does not do the work.
- **No retries.** Committed tokens are never generated again. A stream stores the prompt, model and tokenizer
  hashes, sampler settings, token IDs and generator state, and restoring it rebuilds the model's context. Hosted
  models cannot do this, so agents use local models.
- **Clients only see committed tokens.** A crash can only lose tokens no client has seen.
- **Scripts are replayed.** An isolate cannot be moved. Every op result is logged, and a new node re-runs the
  script against those results. This costs CPU but no tokens, and repeats no writes. Scripts must be deterministic,
  so time and randomness come through logged ops.
- **External actions are never blindly repeated.** The intent is logged first. If a node dies during the action,
  the outcome is `unknown` and is checked or escalated.
- **Fencing.** A writer names the sequence and lease it holds. A stale writer is refused.

## Redundancy

New data must be stored as full copies. RS(3,2) cannot protect it on its own: with `n=5, k=3`, the rule
`Qw + Qr - n >= k` forces quorums of 4, which tolerates only one failure.

1. New tokens and task results are copied through Raft and committed once 3 nodes hold them.
2. Sealed segments are encoded as RS(3,2). Full copies are removed only after all 5 fragments are confirmed on
   separate nodes. Every fragment has a checksum.

With 3 nodes left, the cluster shrinks its membership to 3 and re-encodes as RS(2,1), so it can survive one more
failure after that finishes. The shrink happens once two nodes have been silent for `CLUSTER.SHRINK_AFTER` seconds
(default 600). It is suspicion-based, because no timeout tells slow from dead, and permanent in this version: a
node that was only unreachable cannot rejoin, and the cluster stays at 3 members with one tolerated failure.

## Request deadline and no quorum

Every clustered write and read barrier runs under one deadline of about 3 s (`REQUEST_DEADLINE`, D024). Inside
it a request retries, after a pause of about 40 ms, only failures known to be before commit: no known leader, a
refused connection to the cached leader, and a leader that cannot reach a quorum. When the deadline expires, or
the last such failure persists, the node answers `503 {"error":"no_quorum"}`, which differs from `unavailable`
(no leader known yet). Only `no_quorum` is certain: nothing that answered it is visible after recovery.

An isolated leader never steps down in openraft 0.9, so the leader confirms its quorum before it appends. That
check-then-append is a documented race: a quorum lost between the check and the append leaves an entry in the
leader's log that commits once the quorum returns. A write whose outcome is unknown in this way answers
`503 {"error":"outcome_unknown"}`, never `no_quorum`. That happens when the deadline expires after the append
was handed to Raft, and when a forward to the leader was sent and gets no answer (timeout or lost connection).
`outcome_unknown` is never retried inside the deadline: the entry may commit later, so the client checks the
store before it retries. A refused connection to the leader is known to be before the append, so it stays
`no_quorum`.

## Node loss and reassignment

The leader treats a node as lost after about 1 s without a reply to its heartbeats, and it checks every 100 ms.
After an election the new leader therefore reassigns a lost node's goals within about 1 s of silence plus one
100 ms tick. It gives each such goal to the least-loaded live node with an `Assign` command that is a
compare-and-set on the goal's lease token: it carries the token it read and commits only if that is still the
current token, so two reassignments cannot both win.

A node that returns, or is un-paused with SIGCONT, runs nothing it lost. The new `Assign` raised the token, so
the old owner's commits are refused as fenced when they are applied, whatever the node believes about itself, and
its worker stops without an error. If the old owner proposes a `Claim` for the goal, it is refused as
`not_owner`. A paused old leader cannot commit its proposals either, because it holds an older term.

## Five nodes

A 5-node cluster has quorum 3, so it tolerates 2 lost nodes, including the leader: writes continue and every
goal finishes byte-identical. With 3 nodes down the 2 survivors answer `503 no_quorum` within the deadline and
commit nothing; once one node returns, work completes. Goals spread over live nodes, so 5 or more goals run on
at least 3 nodes at once.

## Build order

1. Durable log and store on one node. **Done:** `src/journal.rs`, `src/store/mod.rs`.
2. Scheduler and resumable agent on one node, using a deterministic fake model. **Done:** `src/goals.rs`, `src/lease.rs`, `src/model.rs`, `src/http/goals.rs`.
3. Raft on 3 nodes with several workers, then 5. **Done:** `CLUSTER.NODE_ID` and `CLUSTER.PEERS` start a cluster; `tests/cluster.rs` covers 3 and 5 nodes, and `scripts/ci-smoke.sh` kills the leader of a 3-node release cluster.
4. Reed-Solomon sealing and repair. **Done:** `src/seal.rs`, `src/raft/seal.rs`, `src/raft/assign.rs` (shrink to the 3 live voters and RS(2,1) re-encode, after the configurable CLUSTER.SHRINK_AFTER); `tests/sealing.rs` covers acceptance 1-7, and `scripts/ci-smoke.sh` seals a goal on a 5-node release cluster and reads it after two kills.
5. T3 Code provider adapter. **Done:** `src/acp.rs`, `src/sessions.rs`, `tests/acp.rs`, the `acp_` tests in `tests/cluster.rs`, and the ACP section of `scripts/ci-smoke.sh`. T3 Code is the control surface: `rs3` owns agent state and failover.

JavaScript engine replacement (V8, Boa, Wasmtime) and real model inference are separate later decisions. Keep both
behind small interfaces.
