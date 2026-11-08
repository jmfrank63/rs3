# Milestone: Reed-Solomon sealing, repair and shrink

Read `AGENTS.md` and `docs/distributed-design.md` first, especially "Redundancy". This is step 4 of the build
order. Step 3 is done: 3 to 5 nodes form one Raft cluster (`src/raft/`), every goal stream and op log is a full
copy on every node, and the Raft log is never snapshotted or purged.

## Goal

Finished data stops costing a full copy per node. An ended goal's stream is sealed into RS(3,2) fragments, one
per node, each with a checksum, so the cluster stores about 5/3 of the data instead of 5 full copies. Any 3
fragments rebuild it, so sealed data survives the loss of any 2 of 5 nodes. When 2 nodes stay lost, the cluster
shrinks to 3 members and re-encodes as RS(2,1), so it survives one more failure.

## Required behaviour

- **Seal ended streams.** When a goal's stream ends, the leader seals it: it encodes the stream into 5 fragments,
  and each node stores its own fragment with a checksum. Live goals and the key/value store stay full copies.
- **Full copies go last.** A node removes its full copy only after Raft has committed that all 5 fragments are
  confirmed on separate nodes. A node lost during sealing leaves every full copy in place until sealing completes.
- **Reads stay the same.** `GET /goals/{id}` returns byte-identical output for a sealed goal on every node,
  rebuilding from any 3 fragments when needed. A fragment that fails its checksum is never used.
- **Repair.** A fragment that is missing or fails its checksum is rebuilt from the others and stored again.
- **The Raft log forgets.** Snapshots and log purging are turned on, so sealed data's full copies also leave the
  Raft log. A node that restarts after the purge catches up from a snapshot.
- **Shrink.** When 2 of 5 nodes stay unreachable longer than a named timeout, the leader removes them from Raft
  membership, and sealed streams are re-encoded as RS(2,1) across the 3 members. Until re-encoding finishes, the
  RS(3,2) fragments stay in place. With 3 members, the cluster keeps working after one more loss.
- **Crate choice.** Prefer `reed-solomon-simd` 3.x (maintained, pure Rust) unless research finds a blocker. Record
  the decision and why. No unmaintained crates.
- **Observable storage.** A small read-only endpoint, for example `GET /goals/{id}/storage`, shows on each node
  whether it holds a full copy, a fragment (with its index and length) or nothing. Tests use it instead of
  reaching into redb.

## Acceptance tests

All are behavioural. They start real `rs3` processes on localhost and bound every wait with a deadline:

1. **Sealing:** on 5 nodes, after a goal ends, each node holds one fragment of about a third of the stream's size,
   no node holds a full copy, and the output read through every node is byte-identical to a single-node run.
2. **Two lost after sealing:** `kill -9` any two of five nodes. Every sealed output is still byte-identical
   through each survivor.
3. **Corrupted fragment:** a fragment flipped on disk of a stopped node is detected after restart, never used,
   the output is still correct, and the fragment is repaired.
4. **Lost during sealing:** a node killed while a stream is being sealed leaves full copies in place. Output stays
   correct throughout, and sealing completes after the node returns.
5. **Purge and catch-up:** after sealing and a snapshot, a node restarted from an empty data directory catches up
   from the snapshot and serves every committed write and every sealed output.
6. **Shrink:** two of five nodes stay down past the timeout. Membership becomes 3, sealed streams are re-encoded as
   RS(2,1), and after one more node is killed the last two still serve every sealed output and accept writes.
7. **Repair after return:** a node that missed sealing while it was down gets its fragment once it returns.

The release smoke (`scripts/ci-smoke.sh`) also seals one goal on a 5-node release cluster and reads it back after
killing two nodes.

## Out of scope

Growing membership back after a shrink, sealing the key/value store or live streams, Docker and libvirt nodes,
power-loss tests, real models, external actions, the T3 Code adapter, authentication and TLS.

Prefer the shortest design that satisfies these tests. Split it into small slices, each with a demonstrable
behaviour. Start with encoding and decoding a stream on one node, then sealing on 5 nodes, then repair, then the
Raft snapshot and purge, then shrink.
