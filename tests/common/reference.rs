//! A goal run to the end on a single-node server, to compare a cluster's result with.

use std::time::Duration;

use crate::cluster::wait_until;
use crate::common::{Server, TempDir};

const WAIT: Duration = Duration::from_secs(60);

pub fn goal_body(seed: u64) -> String {
    format!(r#"{{"priority":0,"budget":900,"seed":{seed},"prompt":"p"}}"#)
}

pub fn output(goal: &serde_json::Value) -> String {
    goal["output"].as_str().unwrap().to_owned()
}

/// The output of a goal run to the end on a single-node server.
pub fn single_node_output(seed: u64) -> String {
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = TempDir::new(&format!("cluster-reference-{call}"));
    let server = Server::start(&dir.0, Some(&dir.0.join("rs3.db")), &[]);
    let created: serde_json::Value =
        serde_json::from_str(&server.request("POST", "/goals", &goal_body(seed))).unwrap();
    let path = format!("/goals/{}", created["id"]);
    wait_until("the reference goal", WAIT, || {
        let goal: serde_json::Value =
            serde_json::from_str(&server.request("GET", &path, "")).unwrap();
        (goal["status"] == "done").then(|| output(&goal))
    })
}
