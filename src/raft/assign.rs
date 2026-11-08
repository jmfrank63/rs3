//! The leader's assignment loop: hands every unassigned live goal to a node.
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use std::path::PathBuf;

use super::network::RPC_TIMEOUT;
use super::{BoxError, Node, REQUEST_DEADLINE, seal};
use crate::goals::Goals;
use crate::store::Command;

/// How often the leader looks for goals without an owner.
const TICK: Duration = Duration::from_millis(100);

/// How long a peer may stay silent before the leader moves its goals elsewhere: well above the
/// RPC timeout and the election timeout, so a slow but live peer keeps its goals.
const SILENCE: Duration = RPC_TIMEOUT.saturating_mul(2);

/// The voters left when exactly two of five are lost.
fn survivors(voters: &[u64], lost: &[u64]) -> Option<BTreeSet<u64>> {
    (voters.len() == 5 && lost.len() == 2).then(|| {
        voters
            .iter()
            .copied()
            .filter(|v| !lost.contains(v))
            .collect()
    })
}

/// Shrink a five-voter cluster to its live voters when exactly two stay lost.
async fn shrink(node: &Node, voters: &[u64], lost: &[u64]) {
    let Some(live) = survivors(voters, lost) else {
        return;
    };
    let change = node.raft().change_membership(live, false);
    let done: Result<_, BoxError> =
        async { Ok(tokio::time::timeout(REQUEST_DEADLINE, change).await??) }.await;
    if let Err(e) = done {
        eprintln!("error: shrinking membership: {e}");
    }
}

/// Submit `plan` here, leader-local: a node that no longer leads refuses, so a stale plan commits
/// nothing. A refusal is fine; a later tick replans.
pub(super) async fn submit(node: &Node, plan: Vec<Command>) {
    for cmd in plan {
        let _ = node.write(cmd).await;
    }
}

/// On the Raft runtime of `node` (node `me`), while it leads, assign `goals`' unassigned goals
/// across the current voters. With a `fragments` directory it also seals ended streams, and stores this
/// node's fragment of every seal it belongs to; without one nothing is sealed. A refused assignment is fine: the plan is a compare-and-set that a later
/// tick replans. Two peers silent for `shrink_after` are dropped from the voters for good: the
/// delay is a suspicion, not proof, so it should be far above any outage worth riding out. The
/// loop ends when the node shuts down.
pub fn spawn(
    node: Arc<Node>,
    goals: Goals,
    me: u64,
    fragments: Option<PathBuf>,
    shrink_after: Duration,
) {
    let handle = node.handle();
    handle.spawn(async move {
        let mut metrics = node.raft().metrics();
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            if let Some(dir) = &fragments {
                seal::sweep(&node, goals.journal(), dir, me).await;
            }
            if metrics.has_changed().is_err() {
                return;
            }
            if metrics.borrow_and_update().current_leader != Some(me) {
                continue;
            }
            let voters: Vec<u64> = metrics
                .borrow()
                .membership_config
                .membership()
                .voter_ids()
                .collect();
            shrink(&node, &voters, &node.lost_peers(shrink_after)).await;
            let lost = node.lost_peers(SILENCE);
            let live: Vec<u64> = voters
                .iter()
                .copied()
                .filter(|m| !lost.contains(m))
                .collect();
            match goals.plan_assignments(&live) {
                Ok(plan) => submit(&node, plan).await,
                Err(e) => eprintln!("error: planning assignments: {e}"),
            }
            if let Some(dir) = &fragments {
                seal::propose(&node, goals.journal(), dir, me).await;
            }
        }
    });
}
