# Milestone: Raft cluster of 3 to 5 nodes

Read `AGENTS.md` and `docs/distributed-design.md` first. This is step 3 of the build order. Step 2 is done: one
process runs goals that survive `kill -9`, through `src/journal.rs`, `src/goals.rs`, `src/lease.rs` and
`src/js/oplog.rs`. The lease with its fencing token was built as the seam for this milestone.

## Goal

3 to 5 `rs3` processes form one cluster. It presents itself as one database and one agent, and keeps working when
a minority of nodes is lost: 1 of 3, or 2 of 5. Every node runs goals, so no node is a bystander. A goal whose node
dies continues on another node from its replicated state. No committed token is generated again, and its output is
byte-identical to an undisturbed run.

## Required behaviour

- **Raft orders everything.** Store writes, goal creation and cancel, lease claims, and journal appends are Raft
  log entries. A state machine applies them to redb on every node. Fencing is checked when an entry is applied, so a
  stale writer is refused on every node in the same way.
- **One database.** A client may talk to any node. Writes reach the leader, and reads see every committed write.
  When no majority is reachable, requests fail with a clear error within a bounded time and commit nothing.
- **One agent, all nodes working.** Goals are assigned to nodes through Raft. Each node runs its goals with its own
  workers, and proposes token batches and op results under the goal's fence. When a node is lost, its goals are
  reassigned with a new fence and resume from the replicated journal. A returning node gets no work it has lost.
- **Scripts across nodes.** A script goal moved to another node is replayed against its logged op results, so each
  store write still happens exactly once.
- **Static membership.** Nodes come from configuration (node id and peer addresses). Without cluster configuration
  `rs3` is a single node, and the existing tests keep passing unchanged.
- **Crate choice.** Prefer a maintained Raft crate over writing Raft: `openraft` 0.9.x stable, or `raft` (TiKV)
  if research finds a reason. No alpha releases. Reuse redb for the Raft log, and actix for transport where it fits.
  Record the decision and why.

## Acceptance tests

All are behavioural. They start real `rs3` processes on localhost, each with its own data directory and port, and
bound every wait with a deadline:

1. **One database:** on 3 nodes, a key written through any node is read back through every node.
2. **Leader loss:** on 3 nodes, `kill -9` the leader during generation goals. Every goal finishes, its output is
   byte-identical to an undisturbed run, and its journal holds each token batch exactly once.
3. **Two of five lost:** on 5 nodes, `kill -9` two nodes, one of them the leader, while goals run. The cluster keeps
   accepting writes and goals, and every goal finishes with byte-identical output.
4. **No majority:** on 5 nodes, stop three. The two left refuse writes with a clear error within the deadline and
   commit nothing. After one node returns, work continues and outputs are still correct.
5. **No bystanders:** on 5 nodes with at least 5 goals, goals run on at least 3 different nodes at once, as goal
   status shows.
6. **Stale leader:** `SIGSTOP` the leader while it runs goals, let the others elect a new leader and reassign the
   work, then `SIGCONT` it. Nothing the old leader proposes is committed, and every output is still correct.
7. **Script failover:** `kill -9` the node running a store-writing script. The script finishes on another node and
   each write happened exactly once.
8. **Catch-up:** a node that was killed and restarted catches up, and serves every committed write.

The release smoke (`scripts/ci-smoke.sh`) also starts a 3-node cluster, kills the leader and checks one generation
goal and one script goal.

## Out of scope

Reed-Solomon, membership changes (shrinking 5 to 3 is part of the Reed-Solomon milestone), Docker and libvirt
nodes, power-loss tests, real models, external actions, the T3 Code adapter, authentication and TLS.

Prefer the shortest design that satisfies these tests. Split it into small slices, each with a demonstrable
behaviour. Start with a 3-node cluster that replicates the store, then move goals and leases onto Raft, then 5 nodes.
