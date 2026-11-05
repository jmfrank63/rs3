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
failure after that finishes.

## Build order

1. Durable log and store on one node. **Done:** `src/journal.rs`, `src/store/mod.rs`.
2. Scheduler and resumable agent on one node, using a deterministic fake model.
3. Raft on 3 nodes with several workers, then 5.
4. Reed-Solomon sealing and repair.
5. T3 Code provider adapter. T3 Code is the control surface: `rs3` owns agent state and failover.

JavaScript engine replacement (V8, Boa, Wasmtime) and real model inference are separate later decisions. Keep both
behind small interfaces.
