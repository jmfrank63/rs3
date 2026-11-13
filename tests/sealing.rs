//! An ended goal's stream is sealed into one checksummed fragment per node of a 5-node cluster.

#[cfg(test)]
#[path = "common/cluster.rs"]
mod cluster;
#[cfg(test)]
mod common;
#[cfg(test)]
#[path = "common/reference.rs"]
mod reference;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::cluster::{Cluster, wait_until};
    use crate::reference::{goal_body, output, single_node_output};

    const WAIT: Duration = Duration::from_secs(60);
    const SLOW: &[(&str, &str)] = &[("MODEL.TOKENS_PER_SECOND", "60")];

    fn holds(cluster: &Cluster, node: usize, id: u64) -> serde_json::Value {
        let path = format!("/goals/{id}/storage");
        let (status, reply) = wait_until("storage to answer", WAIT, || {
            let (status, reply) = cluster.request_status(node, "GET", &path, "");
            (status != 503).then_some((status, reply))
        });
        assert_eq!(status, 200, "{reply}");
        serde_json::from_str(&reply).expect(&reply)
    }

    fn done(cluster: &Cluster, node: usize, id: u64) -> serde_json::Value {
        wait_until("the goal to finish", WAIT, || {
            let goal = cluster.goal(node, id);
            (goal["status"] == "done").then_some(goal)
        })
    }

    #[test]
    fn every_node_keeps_one_third_of_a_sealed_stream_and_survives_two_kills() {
        let mut cluster = Cluster::start("sealing-five", 5);
        let id = cluster.create_goal(0, &goal_body(1));
        let expected = single_node_output(1);
        assert_eq!(output(&done(&cluster, 0, id)), expected);
        let mut indexes = wait_until("all five nodes to hold a fragment", WAIT, || {
            let held: Vec<serde_json::Value> =
                (0..cluster.len()).map(|n| holds(&cluster, n, id)).collect();
            held.iter()
                .all(|h| h["holds"] == "fragment")
                .then_some(held)
        })
        .iter()
        .map(|h| {
            assert_eq!(
                (h["data"].as_u64(), h["parity"].as_u64()),
                (Some(3), Some(2))
            );
            h["index"].as_u64().unwrap()
        })
        .collect::<Vec<_>>();
        indexes.sort_unstable();
        assert_eq!(indexes, [0, 1, 2, 3, 4]);
        let len = holds(&cluster, 0, id)["len"].as_u64().unwrap();
        for node in 0..5 {
            let size = std::fs::metadata(
                cluster
                    .dir
                    .0
                    .join(format!("node{}.db.fragments/{id}", node + 1)),
            )
            .unwrap()
            .len();
            assert!(
                size.abs_diff(len / 3) <= 64 && size < len / 2,
                "node {node}: {size} of {len}"
            );
            assert_eq!(output(&cluster.goal(node, id)), expected, "node {node}");
        }
        let leader = cluster.leader();
        let follower = (leader + 1) % 5;
        cluster.kill(leader);
        cluster.kill(follower);
        for node in (0..cluster.len()).filter(|n| *n != leader && *n != follower) {
            assert_eq!(output(&cluster.goal(node, id)), expected, "node {node}");
        }
    }

    #[test]
    fn two_lost_nodes_are_dropped_and_the_survivors_seal_as_two_data_one_parity() {
        let mut cluster = Cluster::start("sealing-shrink", 5);
        let leader = cluster.leader();
        let (a, b) = ((leader + 1) % 5, (leader + 2) % 5);
        cluster.kill(a);
        cluster.kill(b);
        let survivors: Vec<usize> = (0..cluster.len()).filter(|n| *n != a && *n != b).collect();
        // Let the leader drop the two lost voters first; a seal planned earlier stays over five.
        std::thread::sleep(Duration::from_secs(9));
        let id = cluster.create_goal(leader, &goal_body(3));
        let expected = single_node_output(3);
        for &node in &survivors {
            let held = wait_until("a 2+1 fragment", WAIT, || {
                let h = holds(&cluster, node, id);
                (h["holds"] == "fragment").then_some(h)
            });
            assert_eq!(
                (held["data"].as_u64(), held["parity"].as_u64()),
                (Some(2), Some(1)),
                "node {node}"
            );
            assert_eq!(output(&done(&cluster, node, id)), expected, "node {node}");
        }
        let reply = cluster.request_when_available(
            survivors[0],
            "POST",
            "/insert",
            r#"{"after":"shrink"}"#,
        );
        assert!(!reply.contains("error"), "{reply}");
    }

    #[test]
    fn two_nodes_silent_for_less_than_the_shrink_delay_keep_their_membership() {
        let mut cluster =
            Cluster::start_with("sealing-blip", 5, &[("CLUSTER.SHRINK_AFTER", "600")]);
        let leader = cluster.leader();
        let (a, b) = ((leader + 1) % 5, (leader + 2) % 5);
        cluster.kill(a);
        cluster.kill(b);
        let id = cluster.create_goal(leader, &goal_body(3));
        let survivors: Vec<usize> = (0..cluster.len()).filter(|n| *n != a && *n != b).collect();
        for &node in &survivors {
            done(&cluster, node, id);
        }
        // Well past the 5 s the other tests wait before a shrink: the seal over five voters needs
        // the two silent ones, so the survivors still hold the whole stream.
        std::thread::sleep(Duration::from_secs(12));
        for &node in &survivors {
            assert_eq!(holds(&cluster, node, id)["holds"], "full", "node {node}");
        }
    }

    #[test]
    fn a_stream_sealed_over_five_is_resealed_over_the_survivors_after_the_shrink() {
        let mut cluster = Cluster::start("sealing-reseal", 5);
        let leader = cluster.leader();
        let (a, b) = ((leader + 1) % 5, (leader + 2) % 5);
        cluster.kill(a);
        cluster.kill(b);
        let survivors: Vec<usize> = (0..cluster.len()).filter(|n| *n != a && *n != b).collect();
        let id = cluster.create_goal(leader, &goal_body(3));
        let expected = single_node_output(3);
        for &node in &survivors {
            assert_eq!(output(&done(&cluster, node, id)), expected, "node {node}");
        }
        for &node in &survivors {
            let held = wait_until("a 2+1 fragment", WAIT, || {
                let h = holds(&cluster, node, id);
                (h["holds"] == "fragment" && h["data"] == 2).then_some(h)
            });
            assert_eq!(held["parity"].as_u64(), Some(1), "node {node}");
            assert_eq!(output(&done(&cluster, node, id)), expected, "node {node}");
        }
    }

    #[test]
    fn a_third_kill_after_the_reseal_leaves_two_nodes_serving_every_sealed_output() {
        let mut cluster = Cluster::start("sealing-third-kill", 5);
        let leader = cluster.leader();
        let id = cluster.create_goal(leader, &goal_body(3));
        let expected = single_node_output(3);
        for node in 0..cluster.len() {
            wait_until("a fragment", WAIT, || {
                (holds(&cluster, node, id)["holds"] == "fragment").then_some(())
            });
        }
        let (a, b) = ((leader + 1) % 5, (leader + 2) % 5);
        cluster.kill(a);
        cluster.kill(b);
        let mut survivors: Vec<usize> = (0..cluster.len()).filter(|n| *n != a && *n != b).collect();
        for &node in &survivors {
            let held = wait_until("a 2+1 fragment", WAIT, || {
                let h = holds(&cluster, node, id);
                (h["holds"] == "fragment" && h["data"] == 2).then_some(h)
            });
            assert_eq!(held["parity"].as_u64(), Some(1), "node {node}");
            assert_eq!(output(&done(&cluster, node, id)), expected, "node {node}");
        }
        cluster.kill(survivors.pop().unwrap());
        for &node in &survivors {
            let goal = wait_until("the output from two nodes", WAIT, || {
                let (status, reply) =
                    cluster.request_status(node, "GET", &format!("/goals/{id}"), "");
                (status == 200).then(|| serde_json::from_str::<serde_json::Value>(&reply).unwrap())
            });
            assert_eq!(output(&goal), expected, "node {node}");
        }
        let reply =
            cluster.request_when_available(survivors[0], "POST", "/insert", r#"{"after":"third"}"#);
        assert!(!reply.contains("error"), "{reply}");
    }

    #[test]
    fn a_node_lost_during_sealing_keeps_full_copies_until_it_returns() {
        let mut cluster = Cluster::start_with("sealing-lost", 5, SLOW);
        let id = cluster.create_goal(0, &goal_body(2));
        let expected = single_node_output(2);
        let leader = cluster.leader();
        let victim = (leader + 1) % 5;
        cluster.kill(victim);
        let survivors: Vec<usize> = (0..cluster.len()).filter(|n| *n != victim).collect();
        for &node in &survivors {
            assert_eq!(output(&done(&cluster, node, id)), expected, "node {node}");
        }
        // Sealing is proposed, but the lost member never confirms: full copies stay.
        std::thread::sleep(Duration::from_secs(3));
        for &node in &survivors {
            assert_eq!(holds(&cluster, node, id)["holds"], "full", "node {node}");
            assert_eq!(output(&cluster.goal(node, id)), expected, "node {node}");
        }
        cluster.restart(victim);
        wait_until("all five nodes to hold a fragment", WAIT, || {
            (0..cluster.len())
                .all(|n| holds(&cluster, n, id)["holds"] == "fragment")
                .then_some(())
        });
        for node in 0..5 {
            assert_eq!(output(&cluster.goal(node, id)), expected, "node {node}");
        }
    }

    #[test]
    fn a_fragment_flipped_on_a_stopped_node_is_never_used_and_is_repaired() {
        let mut cluster = Cluster::start("sealing-flipped", 5);
        let id = cluster.create_goal(0, &goal_body(4));
        let expected = single_node_output(4);
        assert_eq!(output(&done(&cluster, 0, id)), expected);
        wait_until("all five nodes to hold a fragment", WAIT, || {
            (0..cluster.len())
                .all(|n| holds(&cluster, n, id)["holds"] == "fragment")
                .then_some(())
        });
        let victim = (cluster.leader() + 1) % 5;
        let recorded = holds(&cluster, victim, id);
        let path = cluster
            .dir
            .0
            .join(format!("node{}.db.fragments/{id}", victim + 1));
        let bytes = std::fs::read(&path).unwrap();
        cluster.kill(victim);
        let mut flipped = bytes.clone();
        flipped[bytes.len() / 2] ^= 0xff;
        std::fs::write(&path, &flipped).unwrap();
        cluster.restart(victim);
        for node in 0..cluster.len() {
            let goal = wait_until("the goal to be readable", WAIT, || {
                let goal = cluster.goal(node, id);
                (goal["status"] == "done").then_some(goal)
            });
            assert_eq!(output(&goal), expected, "node {node}");
        }
        wait_until("the fragment to be repaired", WAIT, || {
            let now = holds(&cluster, victim, id);
            (now["holds"] == "fragment"
                && now["index"] == recorded["index"]
                && now["len"] == recorded["len"]
                && std::fs::read(&path).ok().as_deref() == Some(bytes.as_slice()))
            .then_some(())
        });
    }

    #[test]
    fn a_node_down_before_the_goal_ends_gets_its_fragment_when_it_returns() {
        let mut cluster = Cluster::start("sealing-down-first", 5);
        let down = (cluster.leader() + 1) % 5;
        cluster.kill(down);
        let live: Vec<usize> = (0..cluster.len()).filter(|n| *n != down).collect();
        let id = cluster.create_goal(live[0], &goal_body(5));
        let expected = single_node_output(5);
        for &node in &live {
            assert_eq!(output(&done(&cluster, node, id)), expected, "node {node}");
        }
        std::thread::sleep(Duration::from_secs(3));
        let held: Vec<_> = live.iter().map(|&n| holds(&cluster, n, id)).collect();
        assert!(held.iter().any(|h| h["holds"] == "full"), "{held:?}");
        assert!(held.iter().all(|h| h["holds"] != "none"), "{held:?}");
        cluster.restart(down);
        wait_until("all five nodes to hold a fragment", WAIT, || {
            (0..cluster.len())
                .all(|n| holds(&cluster, n, id)["holds"] == "fragment")
                .then_some(())
        });
        for node in 0..cluster.len() {
            assert_eq!(output(&cluster.goal(node, id)), expected, "node {node}");
        }
    }

    #[test]
    fn a_node_restarted_from_an_empty_directory_catches_up_from_a_snapshot_and_repairs_its_fragment()
     {
        let mut cluster = Cluster::start("sealing-empty-restart", 5);
        let write = |key: String| {
            let reply = cluster.request_when_available(
                0,
                "POST",
                "/insert",
                &format!(r#"{{"{key}":"value-{key}"}}"#),
            );
            assert!(!reply.contains("error"), "{reply}");
        };
        let early: Vec<String> = (0..5).map(|n| format!("early{n}")).collect();
        early.iter().cloned().for_each(write);
        let id = cluster.create_goal(0, &goal_body(6));
        let expected = single_node_output(6);
        assert_eq!(output(&done(&cluster, 0, id)), expected);
        wait_until("all five nodes to hold a fragment", WAIT, || {
            (0..cluster.len())
                .all(|n| holds(&cluster, n, id)["holds"] == "fragment")
                .then_some(())
        });
        let sealed = cluster.request_when_available(0, "GET", &format!("/goals/{id}"), "");
        let victim = (cluster.leader() + 1) % 5;
        let fragment = cluster
            .dir
            .0
            .join(format!("node{}.db.fragments/{id}", victim + 1));
        let original = std::fs::read(&fragment).unwrap();
        // More writes than a snapshot interval, so a snapshot is taken and the log purged.
        let late: Vec<String> = (0..120).map(|n| format!("late{n}")).collect();
        late.iter().cloned().for_each(write);
        cluster.kill(victim);
        let node = format!("node{}.db", victim + 1);
        std::fs::remove_file(cluster.dir.0.join(&node)).unwrap();
        std::fs::remove_dir_all(cluster.dir.0.join(format!("{node}.fragments"))).unwrap();
        cluster.restart(victim);
        let path = format!("/goals/{id}/storage");
        wait_until("the restored node to repair its fragment", WAIT, || {
            let (status, reply) = cluster.request_status(victim, "GET", &path, "");
            (status == 200 && reply.contains(r#""holds":"fragment""#)).then_some(())
        });
        wait_until("the fragment to be rebuilt on disk", WAIT, || {
            (std::fs::read(&fragment).ok().as_deref() == Some(original.as_slice())).then_some(())
        });
        for key in early.iter().chain(&late) {
            let got = wait_until("the restored node to serve a key", WAIT, || {
                let (status, reply) =
                    cluster.request_status(victim, "GET", &format!("/key/{key}"), "");
                (status == 200).then_some(reply)
            });
            assert_eq!(got, format!(r#""value-{key}""#));
        }
        let restored = wait_until("the sealed output on the restored node", WAIT, || {
            let (status, reply) =
                cluster.request_status(victim, "GET", &format!("/goals/{id}"), "");
            (status == 200).then_some(reply)
        });
        assert_eq!(restored, sealed);
    }

    #[test]
    fn a_paused_node_keeps_full_copies_until_it_resumes() {
        let mut cluster = Cluster::start_with("sealing-paused", 5, SLOW);
        let id = cluster.create_goal(0, &goal_body(3));
        let expected = single_node_output(3);
        let paused = (cluster.leader() + 1) % cluster.len();
        cluster.stop(paused);
        let others: Vec<usize> = (0..cluster.len()).filter(|n| *n != paused).collect();
        for &node in &others {
            assert_eq!(output(&done(&cluster, node, id)), expected, "node {node}");
        }
        std::thread::sleep(Duration::from_secs(3));
        for &node in &others {
            assert_eq!(holds(&cluster, node, id)["holds"], "full", "node {node}");
        }
        cluster.cont(paused);
        wait_until("all nodes to hold a fragment", WAIT, || {
            (0..cluster.len())
                .all(|n| holds(&cluster, n, id)["holds"] == "fragment")
                .then_some(())
        });
        for node in 0..cluster.len() {
            assert_eq!(output(&cluster.goal(node, id)), expected, "node {node}");
        }
    }
}
