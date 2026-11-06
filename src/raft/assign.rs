//! The leader's assignment loop: hands every unassigned live goal to a node.
use std::sync::Arc;
use std::time::Duration;

use super::Node;
use super::network::RPC_TIMEOUT;
use crate::goals::Goals;
use crate::store::Command;

/// How often the leader looks for goals without an owner.
const TICK: Duration = Duration::from_millis(100);

/// How long a peer may stay silent before the leader moves its goals elsewhere: well above the
/// RPC timeout and the election timeout, so a slow but live peer keeps its goals.
const SILENCE: Duration = RPC_TIMEOUT.saturating_mul(2);

/// Submit `plan` here, leader-local: a node that no longer leads refuses, so a stale plan commits
/// nothing. A refusal is fine; a later tick replans.
pub(super) async fn submit(node: &Node, plan: Vec<Command>) {
    for cmd in plan {
        let _ = node.write(cmd).await;
    }
}

/// On the Raft runtime of `node` (node `me`), while it leads, assign `goals`' unassigned goals
/// across `members`. A refused assignment is fine: the plan is a compare-and-set that a later
/// tick replans. The loop ends when the node shuts down.
pub fn spawn(node: Arc<Node>, goals: Goals, me: u64, members: Vec<u64>) {
    let handle = node.handle();
    handle.spawn(async move {
        let mut metrics = node.raft().metrics();
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            if metrics.has_changed().is_err() {
                return;
            }
            if metrics.borrow_and_update().current_leader != Some(me) {
                continue;
            }
            let lost = node.lost_peers(SILENCE);
            let live: Vec<u64> = members
                .iter()
                .copied()
                .filter(|m| !lost.contains(m))
                .collect();
            match goals.plan_assignments(&live) {
                Ok(plan) => submit(&node, plan).await,
                Err(e) => eprintln!("error: planning assignments: {e}"),
            }
        }
    });
}
