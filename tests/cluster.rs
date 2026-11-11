//! Three real rs3 processes form one Raft cluster that every node reads and writes through.

#[cfg(test)]
#[path = "common/acp.rs"]
mod acp;
#[cfg(test)]
#[path = "common/cluster.rs"]
mod cluster;
#[cfg(test)]
mod common;
#[cfg(test)]
#[path = "common/model.rs"]
mod model;
#[cfg(test)]
#[path = "common/proxy.rs"]
mod proxy;
#[cfg(test)]
#[path = "common/reference.rs"]
mod reference;

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::cluster::{Cluster, wait_until};
    use crate::common::{Server, TempDir, free_port, rs3};
    use crate::reference::{goal_body, output, single_node_output};

    #[test]
    fn a_key_written_through_each_node_is_read_through_every_node() {
        let cluster = Cluster::start("cluster-rw", 3);
        for writer in 0..cluster.len() {
            let key = format!("key-{writer}");
            let body = format!(r#"{{"{key}":"v{writer}"}}"#);
            cluster.request_when_available(writer, "POST", "/insert", &body);
        }
        for reader in 0..cluster.len() {
            for writer in 0..cluster.len() {
                let got = cluster.request_when_available(
                    reader,
                    "GET",
                    &format!("/key/key-{writer}"),
                    "",
                );
                assert_eq!(got, format!(r#""v{writer}""#), "node {reader}");
            }
        }
    }

    #[test]
    fn a_key_is_readable_through_another_node_the_moment_it_is_written() {
        let cluster = Cluster::start("cluster-fresh", 3);
        cluster.request_when_available(0, "POST", "/insert", r#"{"first":"0"}"#);
        let leader = cluster.leader();
        std::thread::scope(|scope| {
            for worker in 0..8 {
                let cluster = &cluster;
                scope.spawn(move || {
                    for i in 0..25 {
                        let key = format!("fresh-{worker}-{i}");
                        let body = format!(r#"{{"{key}":"{key}"}}"#);
                        cluster.request(leader, "POST", "/insert", &body);
                        let reader = (leader + 1 + (worker + i) % 2) % 3;
                        let got = cluster.request(reader, "GET", &format!("/key/{key}"), "");
                        assert_eq!(got, format!(r#""{key}""#), "node {reader}");
                    }
                });
            }
        });
    }

    #[test]
    fn a_delete_through_one_node_returns_the_old_value_and_removes_it_everywhere() {
        let cluster = Cluster::start("cluster-del", 3);
        cluster.request_when_available(0, "POST", "/insert", r#"{"key-1":"old"}"#);
        let missing = Server::start(&TempDir::new("cluster-missing").0, None, &[]).request(
            "GET",
            "/key/key-1",
            "",
        );
        assert!(missing.contains("not_found"), "{missing}");
        let old = cluster.request_when_available(1, "DELETE", "/key/key-1", "");
        assert!(old.contains("old"), "{old}");
        for node in 0..cluster.len() {
            let got = cluster.request_when_available(node, "GET", "/key/key-1", "");
            assert_eq!(got, missing, "node {node}");
        }
    }

    #[test]
    fn a_killed_and_restarted_node_serves_a_write_committed_while_it_was_down() {
        let mut cluster = Cluster::start("cluster-restart", 3);
        let victim = (cluster.leader() + 1) % cluster.len();
        let other = (victim + 1) % cluster.len();
        cluster.request_when_available(other, "POST", "/insert", r#"{"before":"1"}"#);
        cluster.kill(victim);
        cluster.request_when_available(other, "POST", "/insert", r#"{"during":"2"}"#);
        cluster.restart(victim);
        for (key, value) in [("before", "1"), ("during", "2")] {
            let got = cluster.request_when_available(victim, "GET", &format!("/key/{key}"), "");
            assert_eq!(got, format!(r#""{value}""#));
        }
    }

    #[test]
    fn writes_continue_through_the_survivors_after_the_leader_is_killed() {
        let mut cluster = Cluster::start("cluster-leader-kill", 3);
        cluster.request_when_available(0, "POST", "/insert", r#"{"k1":"a"}"#);
        let leader = cluster.leader();
        cluster.kill(leader);
        let survivors: Vec<usize> = (0..cluster.len()).filter(|&i| i != leader).collect();
        for &node in &survivors {
            let body = format!(r#"{{"k2-{node}":"b"}}"#);
            cluster.request_when_available(node, "POST", "/insert", &body);
        }
        for &node in &survivors {
            assert_eq!(
                cluster.request_when_available(node, "GET", "/key/k1", ""),
                r#""a""#
            );
            for &writer in &survivors {
                let got =
                    cluster.request_when_available(node, "GET", &format!("/key/k2-{writer}"), "");
                assert_eq!(got, r#""b""#, "node {node}");
            }
        }
    }

    #[test]
    fn with_two_of_three_down_the_survivor_answers_no_quorum_and_commits_nothing() {
        let mut cluster = Cluster::start("cluster-no-quorum", 3);
        cluster.request_when_available(0, "POST", "/insert", r#"{"k1":"a"}"#);
        let leader = cluster.leader();
        let other = (leader + 1) % cluster.len();
        let survivor = (leader + 2) % cluster.len();
        cluster.kill(leader);
        cluster.kill(other);
        for (method, path, body) in [
            ("POST", "/insert", r#"{"kX":"x"}"#),
            ("DELETE", "/key/k1", ""),
        ] {
            let started = Instant::now();
            let (status, reply) = cluster.request_status(survivor, method, path, body);
            assert!(
                started.elapsed() < Duration::from_secs(6),
                "{method} {path}"
            );
            assert_eq!(status, 503, "{reply}");
            assert!(reply.contains("no_quorum"), "{reply}");
        }
        cluster.restart(leader);
        cluster.restart(other);
        cluster.leader();
        for node in 0..cluster.len() {
            let missing = cluster.request_when_available(node, "GET", "/key/kX", "");
            assert!(missing.contains("not_found"), "node {node}: {missing}");
            let kept = cluster.request_when_available(node, "GET", "/key/k1", "");
            assert_eq!(kept, r#""a""#, "node {node}");
        }
        for node in 0..cluster.len() {
            let body = format!(r#"{{"after-{node}":"y"}}"#);
            let reply = cluster.request_when_available(node, "POST", "/insert", &body);
            assert!(!reply.contains("error"), "node {node}: {reply}");
        }
    }

    #[test]
    fn a_killed_and_restarted_node_serves_every_committed_write() {
        let mut cluster = Cluster::start("cluster-catch-up", 3);
        let victim = (cluster.leader() + 1) % cluster.len();
        let other = (victim + 1) % cluster.len();
        cluster.kill(victim);
        for n in 0..20 {
            let body = format!(r#"{{"bulk-{n}":"{n}"}}"#);
            cluster.request_when_available(other, "POST", "/insert", &body);
        }
        cluster.restart(victim);
        for n in 0..20 {
            let got = cluster.request_when_available(victim, "GET", &format!("/key/bulk-{n}"), "");
            assert_eq!(got, format!(r#""{n}""#));
        }
        let list = cluster.request_when_available(victim, "GET", "/list", "");
        for n in 0..20 {
            let want = format!(r#"{{\"bulk-{n}\" : \"{n}\"}}"#);
            assert!(list.contains(&want), "missing {want} in {list}");
        }
    }

    #[test]
    fn a_node_id_without_peers_stops_with_an_invalid_configuration_error() {
        let dir = TempDir::new("cluster-config");
        let output = rs3(free_port(), &dir.0, None)
            .env("CLUSTER.NODE_ID", "1")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("invalid configuration"), "{stderr}");
    }

    #[test]
    fn a_node_without_cluster_settings_serves_writes_without_waiting_for_peers() {
        let dir = TempDir::new("cluster-single");
        let server = Server::start(&dir.0, None, &[]);
        server.request("POST", "/insert", r#"{"k":"v"}"#);
        assert_eq!(server.request("GET", "/key/k", ""), r#""v""#);
    }

    const RATE: &[(&str, &str)] = &[("MODEL.TOKENS_PER_SECOND", "300")];
    const WAIT: Duration = Duration::from_secs(60);

    #[test]
    fn goals_created_through_different_nodes_run_on_several_nodes_and_match_a_single_node() {
        let cluster = Cluster::start_with("cluster-goals", 3, RATE);
        let ids: Vec<(u64, u64)> = (0..6u64)
            .map(|seed| {
                let id = cluster.create_goal(seed as usize % 3, &goal_body(seed));
                (seed, id)
            })
            .collect();
        wait_until("running goals on two nodes at once", WAIT, || {
            let owners: std::collections::HashSet<String> = ids
                .iter()
                .map(|(_, id)| cluster.goal(0, *id))
                .filter(|g| g["status"] == "running")
                .filter_map(|g| g["node"].as_u64().map(|n| n.to_string()))
                .collect();
            (owners.len() >= 2).then_some(())
        });
        for (seed, id) in ids {
            let done = wait_until("the goal to finish", WAIT, || {
                let goal = cluster.goal(1, id);
                (goal["status"] == "done").then_some(goal)
            });
            let expected = single_node_output(seed);
            for node in 0..3 {
                assert_eq!(output(&cluster.goal(node, id)), expected, "node {node}");
            }
            assert_eq!(output(&done), expected);
        }
    }

    #[test]
    fn a_goal_cancelled_through_a_non_owner_node_stays_cancelled_on_every_node() {
        let cluster = Cluster::start_with("cluster-cancel", 3, RATE);
        let id = cluster.create_goal(0, &goal_body(7));
        let owner = wait_until("a running owned goal with output", WAIT, || {
            let goal = cluster.goal(0, id);
            let started = goal["status"] == "running" && !output(&goal).is_empty();
            started.then(|| goal["node"].as_u64().unwrap() as usize)
        });
        let other = owner % 3;
        let reply = cluster.request_when_available(other, "DELETE", &format!("/goals/{id}"), "");
        let cancelled: serde_json::Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(cancelled["status"], "cancelled", "{reply}");
        let kept = output(&cancelled);
        assert!(single_node_output(7).starts_with(&kept));
        std::thread::sleep(Duration::from_secs(1));
        for node in 0..3 {
            let goal = cluster.goal(node, id);
            assert_eq!(goal["status"], "cancelled", "node {node}");
            assert_eq!(output(&goal), kept, "node {node}");
        }
    }

    #[test]
    fn a_script_goal_created_through_any_node_finishes_and_its_writes_reach_every_node() {
        let cluster = Cluster::start("cluster-script", 3);
        let source = r#"Deno.core.ops.op_rs3_insert('{"a":"1"}');
            const old = Deno.core.ops.op_rs3_delete('a');
            Deno.core.ops.op_rs3_insert('{"b":"' + old + '2"}');
            Deno.core.ops.op_rs3_get('b')"#;
        cluster.request_when_available(
            0,
            "POST",
            "/insert",
            &serde_json::json!({ "job": source }).to_string(),
        );
        for creator in 0..3 {
            let id = cluster.create_goal(creator, r#"{"priority":0,"script":"job"}"#);
            let done = wait_until("the script goal to finish", WAIT, || {
                let goal = cluster.goal(creator, id);
                (goal["status"] == "done").then_some(goal)
            });
            assert_eq!(done["output"], "12", "{done}");
        }
        for node in 0..3 {
            let got = cluster.request_when_available(node, "GET", "/key/b", "");
            assert_eq!(got, r#""12""#, "node {node}");
            let gone = cluster.request_when_available(node, "GET", "/key/a", "");
            assert!(gone.contains("not_found"), "node {node}: {gone}");
        }
    }

    #[test]
    fn a_script_goal_finishes_on_another_node_after_its_node_is_killed_with_each_write_once() {
        let mut cluster = Cluster::start("cluster-script-kill", 3);
        let source = r#"for (let i = 0; i < 5; i++) {
                Deno.core.ops.op_rs3_insert('{"s' + i + '":"' + i + '"}');
                const end = Date.now() + 1000;
                while (Date.now() < end) {}
            }
            String(Object.keys(JSON.parse(Deno.core.ops.op_rs3_list())).length)"#;
        cluster.request_when_available(
            0,
            "POST",
            "/insert",
            &serde_json::json!({ "job": source }).to_string(),
        );
        let id = cluster.create_goal(0, r#"{"priority":0,"script":"job"}"#);
        let victim = wait_until("the script to start writing", WAIT, || {
            let goal = cluster.goal(0, id);
            let first = cluster.request_when_available(0, "GET", "/key/s0", "");
            (goal["status"] == "running" && first == r#""0""#).then(|| {
                goal["node"]
                    .as_u64()
                    .expect("a running goal names its node") as usize
                    - 1
            })
        });
        cluster.kill(victim);
        let live = (victim + 1) % 3;
        let done = wait_until("the script goal to finish", WAIT, || {
            let goal = cluster.goal(live, id);
            (goal["status"] == "done").then_some(goal)
        });
        assert_ne!(done["node"].as_u64(), Some(victim as u64 + 1), "{done}");
        // `job` plus s0..s4, each written once.
        assert_eq!(done["output"], "6", "{done}");
        for node in (0..3).filter(|n| *n != victim) {
            for i in 0..5 {
                let got = cluster.request_when_available(node, "GET", &format!("/key/s{i}"), "");
                assert_eq!(got, format!(r#""{i}""#), "node {node}");
            }
        }
    }

    #[test]
    fn a_cluster_script_sees_a_missing_key_and_reads_through_the_log() {
        let cluster = Cluster::start("cluster-script-missing", 3);
        let source = r#"let m; try { Deno.core.ops.op_rs3_delete('nope') } catch (e) { m = e.message }
            m + '|' + Deno.core.ops.op_rs3_get('x') + '|' + (Deno.core.ops.op_rs3_list().length > 0)"#;
        let body = serde_json::json!({ "job": source, "x": "7" }).to_string();
        cluster.request_when_available(0, "POST", "/insert", &body);
        let id = cluster.create_goal(1, r#"{"priority":0,"script":"job"}"#);
        let done = wait_until("the script goal to finish", WAIT, || {
            let goal = cluster.goal(2, id);
            (goal["status"] == "done").then_some(goal)
        });
        assert_eq!(done["output"], "key not found: nope|7|true", "{done}");
    }

    #[test]
    fn goals_finish_byte_identical_after_the_leader_is_killed() {
        let slow = [("MODEL.TOKENS_PER_SECOND", "100")];
        let mut cluster = Cluster::start_with("cluster-kill-leader", 3, &slow);
        let ids: Vec<(u64, u64)> = (0..4u64)
            .map(|seed| {
                (
                    seed,
                    cluster.create_goal(seed as usize % 3, &goal_body(seed)),
                )
            })
            .collect();
        let leader = cluster.leader();
        wait_until("a goal running on the leader", WAIT, || {
            ids.iter()
                .any(|(_, id)| {
                    let goal = cluster.goal(0, *id);
                    goal["status"] == "running" && goal["node"].as_u64() == Some(leader as u64 + 1)
                })
                .then_some(())
        });
        cluster.kill(leader);
        let survivors: Vec<usize> = (0..3).filter(|n| *n != leader).collect();
        for (seed, id) in &ids {
            let done = wait_until("the goal to finish", WAIT, || {
                let goal = cluster.goal(survivors[0], *id);
                (goal["status"] == "done").then_some(goal)
            });
            let expected = single_node_output(*seed);
            assert_eq!(output(&done), expected);
            assert_ne!(done["node"].as_u64(), Some(leader as u64 + 1));
            assert_eq!(output(&cluster.goal(survivors[1], *id)), expected);
        }
        cluster.restart(leader);
        for (seed, id) in &ids {
            let goal = wait_until("the restarted node to serve the goal", WAIT, || {
                let goal = cluster.goal(leader, *id);
                (goal["status"] == "done").then_some(goal)
            });
            assert_eq!(output(&goal), single_node_output(*seed));
        }
    }

    #[test]
    fn a_paused_leader_that_resumes_commits_nothing_and_every_output_is_correct() {
        let mut cluster = Cluster::start_with("cluster-stop-leader", 3, RATE);
        let ids: Vec<(u64, u64)> = (0..4u64)
            .map(|seed| {
                (
                    seed,
                    cluster.create_goal(seed as usize % 3, &goal_body(seed)),
                )
            })
            .collect();
        let old = cluster.leader();
        let old_id = old as u64 + 1;
        let live: Vec<usize> = (0..3).filter(|n| *n != old).collect();
        wait_until("a goal running on the leader", WAIT, || {
            ids.iter()
                .any(|(_, id)| {
                    let goal = cluster.goal(live[0], *id);
                    goal["status"] == "running" && goal["node"].as_u64() == Some(old_id)
                })
                .then_some(())
        });
        cluster.stop(old);
        wait_until(
            "every goal off the paused node",
            Duration::from_secs(30),
            || {
                ids.iter()
                    .all(|(_, id)| {
                        let goal = cluster.goal(live[0], *id);
                        goal["status"] == "done" || goal["node"].as_u64() != Some(old_id)
                    })
                    .then_some(())
            },
        );
        cluster.cont(old);
        for (seed, id) in &ids {
            let done = wait_until("the goal to finish", WAIT, || {
                let goal = cluster.goal(live[0], *id);
                (goal["status"] == "done").then_some(goal)
            });
            let expected = single_node_output(*seed);
            assert_eq!(output(&done), expected);
            assert_eq!(output(&cluster.goal(live[1], *id)), expected);
            let through_old = wait_until("the old leader to catch up", WAIT, || {
                let goal = cluster.goal(old, *id);
                (goal["status"] == "done").then_some(goal)
            });
            assert_eq!(output(&through_old), expected);
        }
        cluster.request_when_available(old, "POST", "/insert", r#"{"after":"rejoin"}"#);
        for node in 0..3 {
            assert_eq!(
                cluster.request_when_available(node, "GET", "/key/after", ""),
                r#""rejoin""#,
                "node {node}"
            );
        }
    }

    #[test]
    fn two_of_five_lost_including_the_leader_and_every_goal_finishes_byte_identical() {
        let slow = [("MODEL.TOKENS_PER_SECOND", "100")];
        let mut cluster = Cluster::start_with("cluster-five-kill-two", 5, &slow);
        let n = cluster.len();
        let mut ids: Vec<(u64, u64)> = (0..6u64)
            .map(|seed| {
                (
                    seed,
                    cluster.create_goal(seed as usize % n, &goal_body(seed)),
                )
            })
            .collect();
        let leader = cluster.leader();
        let on = |node: usize| Some(node as u64 + 1);
        let other = wait_until("a goal running on a non-leader node", WAIT, || {
            ids.iter().find_map(|(_, id)| {
                let goal = cluster.goal(0, *id);
                let node = (goal["node"].as_u64()? as usize).checked_sub(1)?;
                (goal["status"] == "running" && node != leader).then_some(node)
            })
        });
        cluster.kill(leader);
        cluster.kill(other);
        let survivors: Vec<usize> = (0..n).filter(|i| *i != leader && *i != other).collect();
        cluster.request_when_available(survivors[0], "POST", "/insert", r#"{"five":"ok"}"#);
        for &node in &survivors {
            assert_eq!(
                cluster.request_when_available(node, "GET", "/key/five", ""),
                r#""ok""#,
                "node {node}"
            );
        }
        let late = 6u64;
        ids.push((late, cluster.create_goal(survivors[1], &goal_body(late))));
        for (seed, id) in &ids {
            let done = wait_until(
                &format!("goal {id} (seed {seed}) to finish; killed nodes {leader} and {other}"),
                WAIT,
                || {
                    let goal = cluster.goal(survivors[0], *id);
                    (goal["status"] == "done").then_some(goal)
                },
            );
            assert_eq!(output(&done), single_node_output(*seed), "{done}");
            assert_ne!(done["node"].as_u64(), on(leader), "{done}");
            assert_ne!(done["node"].as_u64(), on(other), "{done}");
        }
    }

    #[test]
    fn with_three_of_five_down_the_survivors_answer_no_quorum_and_commit_nothing() {
        let slow = [("MODEL.TOKENS_PER_SECOND", "100")];
        let mut cluster = Cluster::start_with("cluster-five-no-quorum", 5, &slow);
        let n = cluster.len();
        cluster.request_when_available(0, "POST", "/insert", r#"{"base":"a"}"#);
        let id = cluster.create_goal(0, &goal_body(1));
        wait_until(&format!("goal {id} to run"), WAIT, || {
            (cluster.goal(0, id)["status"] == "running").then_some(())
        });
        let leader = cluster.leader();
        let killed: Vec<usize> = (0..n).map(|k| (leader + k) % n).take(3).collect();
        let survivors: Vec<usize> = (0..n).filter(|i| !killed.contains(i)).collect();
        for &node in &killed {
            cluster.kill(node);
        }
        for &node in &survivors {
            for (method, path, body) in [
                ("POST", "/insert", r#"{"refused":"x"}"#),
                ("DELETE", "/key/base", ""),
            ] {
                let started = Instant::now();
                let (status, reply) = cluster.request_status(node, method, path, body);
                assert!(
                    started.elapsed() < Duration::from_secs(6),
                    "node {node} {method} {path}"
                );
                assert_eq!(status, 503, "node {node}: {reply}");
                assert!(reply.contains("no_quorum"), "node {node}: {reply}");
            }
        }
        cluster.restart(killed[0]);
        cluster.request_when_available(survivors[0], "POST", "/insert", r#"{"after":"y"}"#);
        let late = cluster.create_goal(survivors[1], &goal_body(2));
        for node in survivors.iter().copied().chain([killed[0]]) {
            let missing = cluster.request_when_available(node, "GET", "/key/refused", "");
            assert!(missing.contains("not_found"), "node {node}: {missing}");
            let kept = cluster.request_when_available(node, "GET", "/key/base", "");
            assert_eq!(kept, r#""a""#, "node {node}");
        }
        for (seed, id) in [(1u64, id), (2, late)] {
            let done = wait_until(&format!("goal {id} (seed {seed}) to finish"), WAIT, || {
                let goal = cluster.goal(survivors[0], id);
                (goal["status"] == "done").then_some(goal)
            });
            assert_eq!(output(&done), single_node_output(seed), "{done}");
        }
    }

    #[test]
    fn five_goals_or_more_run_on_at_least_three_nodes_at_once() {
        let slow = [("MODEL.TOKENS_PER_SECOND", "100")];
        let cluster = Cluster::start_with("cluster-five-spread", 5, &slow);
        let ids: Vec<(u64, u64)> = (0..7u64)
            .map(|seed| {
                (
                    seed,
                    cluster.create_goal(seed as usize % cluster.len(), &goal_body(seed)),
                )
            })
            .collect();
        wait_until("running goals on three nodes at once", WAIT, || {
            let owners: std::collections::HashSet<u64> = ids
                .iter()
                .map(|(_, id)| cluster.goal(0, *id))
                .filter(|g| g["status"] == "running")
                .filter_map(|g| g["node"].as_u64())
                .collect();
            (owners.len() >= 3).then_some(())
        });
        for (seed, id) in ids {
            let done = wait_until(&format!("goal {id} (seed {seed}) to finish"), WAIT, || {
                let goal = cluster.goal(1, id);
                (goal["status"] == "done").then_some(goal)
            });
            assert_eq!(output(&done), single_node_output(seed), "{done}");
        }
    }
    fn session_id(cluster: &Cluster, node: usize) -> u64 {
        let reply = cluster.request_when_available(node, "POST", "/sessions", "");
        let created: serde_json::Value = serde_json::from_str(&reply).expect(&reply);
        created["id"].as_u64().expect(&reply)
    }

    fn session_turn(
        cluster: &Cluster,
        node: usize,
        session: u64,
        turn: u64,
        prompt: &str,
    ) -> (u16, String) {
        let body = format!(r#"{{"turn":{turn},"prompt":"{prompt}"}}"#);
        let path = format!("/sessions/{session}/turns");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let (status, reply) = cluster.request_status(node, "POST", &path, &body);
            if status != 503 {
                return (status, reply);
            }
            assert!(
                Instant::now() < deadline,
                "no leader for a session turn: {reply}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn session_goal(cluster: &Cluster, node: usize, session: u64, turn: u64, prompt: &str) -> u64 {
        let (status, reply) = session_turn(cluster, node, session, turn, prompt);
        assert_eq!(status, 201, "{reply}");
        let started: serde_json::Value = serde_json::from_str(&reply).expect(&reply);
        started["goal"].as_u64().expect(&reply)
    }

    fn session_body(cluster: &Cluster, node: usize, session: u64) -> (u16, String) {
        cluster.request_status(node, "GET", &format!("/sessions/{session}"), "")
    }

    #[test]
    fn script_turn_started_through_one_node_resends_to_the_same_goal_and_a_missing_key_records_nothing()
     {
        let cluster = Cluster::start("cluster-script-turn", 3);
        let source = r#"Deno.core.ops.op_rs3_insert('{"w":"1"}'); 'ran'"#;
        cluster.request_when_available(
            0,
            "POST",
            "/insert",
            &serde_json::json!({ "job": source }).to_string(),
        );
        let session = session_id(&cluster, 0);
        let first = session_goal(&cluster, 1, session, 0, "/run job");
        let again = session_goal(&cluster, 2, session, 0, "/run job");
        assert_eq!(first, again);
        let done = wait_until("the script turn to finish", Duration::from_secs(10), || {
            let goal = cluster.goal(0, first);
            (goal["status"] == "done").then_some(goal)
        });
        assert_eq!(done["output"], "ran", "{done}");

        let (status, reply) = session_turn(&cluster, 2, session, 1, "/run nothing");
        assert_eq!(status, 404, "{reply}");
        assert!(reply.contains("not_found"), "{reply}");
        for node in 0..3 {
            let (status, body) = session_body(&cluster, node, session);
            assert_eq!(status, 200);
            let turns: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(turns["turns"].as_array().unwrap().len(), 1, "node {node}");
        }
    }

    #[test]
    fn a_session_created_through_one_node_reads_the_same_through_every_node() {
        let cluster = Cluster::start("cluster-session-read", 3);
        let session = session_id(&cluster, 1);
        let bodies: Vec<_> = (0..3).map(|n| session_body(&cluster, n, session)).collect();
        assert_eq!(bodies[0], (200, r#"{"turns":[]}"#.to_string()));
        assert!(bodies.iter().all(|b| *b == bodies[0]), "{bodies:?}");
    }

    #[test]
    fn two_sessions_created_through_different_nodes_get_different_ids() {
        let cluster = Cluster::start("cluster-session-ids", 3);
        let ids: Vec<u64> = (0..3).map(|n| session_id(&cluster, n)).collect();
        let distinct: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(distinct.len(), 3, "{ids:?}");
    }

    #[test]
    fn a_session_turn_resent_through_another_node_is_one_goal_that_finishes() {
        let cluster = Cluster::start("cluster-session-resend", 3);
        let session = session_id(&cluster, 0);
        let first = session_goal(&cluster, 1, session, 0, "hello");
        let again = session_goal(&cluster, 2, session, 0, "hello");
        assert_eq!(first, again);
        for node in 0..3 {
            let (status, reply) = cluster.request_status(node, "GET", "/goals/2", "");
            assert_eq!(status, 404, "node {node}: {reply}");
        }
        wait_until("the session goal to finish", WAIT, || {
            (cluster.goal(0, first)["status"] == "done").then_some(())
        });
        let bodies: Vec<_> = (0..3).map(|n| session_body(&cluster, n, session)).collect();
        assert!(bodies.iter().all(|b| *b == bodies[0]), "{bodies:?}");
        assert!(bodies[0].1.contains("hello"), "{bodies:?}");
    }

    #[test]
    fn a_refused_session_turn_leaves_the_session_unchanged_on_every_node() {
        let cluster = Cluster::start("cluster-session-refused", 3);
        let session = session_id(&cluster, 0);
        session_goal(&cluster, 0, session, 0, "first");
        let before: Vec<_> = (0..3).map(|n| session_body(&cluster, n, session)).collect();
        let (status, reply) = session_turn(&cluster, 1, session, 0, "other");
        assert_eq!(status, 409, "{reply}");
        assert!(reply.contains("turn_conflict"), "{reply}");
        let (status, reply) = session_turn(&cluster, 2, session, 5, "skipped");
        assert_eq!(status, 409, "{reply}");
        let (status, reply) = session_turn(&cluster, 1, 99, 0, "nobody");
        assert_eq!(status, 404, "{reply}");
        assert!(reply.contains("not_found"), "{reply}");
        let after: Vec<_> = (0..3).map(|n| session_body(&cluster, n, session)).collect();
        assert_eq!(before, after);
        let (status, _) = cluster.request_status(0, "GET", "/goals/2", "");
        assert_eq!(status, 404);
    }

    #[test]
    fn a_node_restarted_from_an_empty_directory_gets_its_sessions_from_a_snapshot() {
        let mut cluster = Cluster::start("cluster-session-snapshot", 3);
        let session = session_id(&cluster, 0);
        let goal = session_goal(&cluster, 0, session, 0, "remember me");
        wait_until("the session goal to finish", WAIT, || {
            (cluster.goal(0, goal)["status"] == "done").then_some(())
        });
        for n in 0..120 {
            let reply = cluster.request_when_available(
                0,
                "POST",
                "/insert",
                &format!(r#"{{"late{n}":"v"}}"#),
            );
            assert!(!reply.contains("error"), "{reply}");
        }
        let expected = session_body(&cluster, 0, session);
        assert_eq!(expected.0, 200);
        let victim = (cluster.leader() + 1) % 3;
        cluster.kill(victim);
        let node = format!("node{}.db", victim + 1);
        std::fs::remove_file(cluster.dir.0.join(&node)).unwrap();
        let _ = std::fs::remove_dir_all(cluster.dir.0.join(format!("{node}.fragments")));
        cluster.restart(victim);
        let restored = wait_until(
            "the restarted node to answer the session",
            Duration::from_secs(20),
            || {
                let reply = session_body(&cluster, victim, session);
                (reply.0 == 200).then_some(reply)
            },
        );
        assert_eq!(restored, expected);
    }

    #[test]
    fn a_lost_turn_response_leaves_one_goal_and_one_session_turn_on_every_node() {
        use serde_json::json;

        use crate::acp::Acp;
        use crate::proxy::Proxy;

        let cluster = Cluster::start_with(
            "cluster-lost-turn",
            3,
            &[("MODEL.TOKENS_PER_SECOND", "500")],
        );
        cluster.leader();
        let proxy = Proxy::start(cluster.ports[0]);
        let urls = [
            format!("http://127.0.0.1:{}", proxy.port),
            format!("http://127.0.0.1:{}", cluster.ports[1]),
        ];
        let mut acp = Acp::start(&cluster.dir.0, &urls);
        let session = acp.new_session(1);
        proxy.drop_next(1);
        let (response, chunks) = acp.prompt(2, &session, &["lost"]);
        assert_eq!(proxy.dropped(), 1);
        assert_eq!(response["result"]["stopReason"], "max_tokens");
        let first = cluster.goal(0, 1);
        assert_eq!(chunks.concat(), output(&first));
        for node in 0..3 {
            let (status, _) = cluster.request_status(node, "GET", "/goals/2", "");
            assert_eq!(status, 404, "node {node}");
            let body = cluster.request_when_available(node, "GET", "/sessions/1", "");
            let body: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(
                body,
                json!({"turns": [{"prompt": "lost", "goal": 1}]}),
                "node {node}"
            );
        }
        assert!(acp.finish().success());
        assert!(acp.rest_of_stdout().is_empty());
    }

    /// The output of turn 0 of session `session` run on a single-node server.
    fn single_node_turn(session: u64, prompt: &str) -> String {
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = TempDir::new(&format!("cluster-turn-reference-{call}"));
        let server = Server::start(&dir.0, Some(&dir.0.join("rs3.db")), &[]);
        loop {
            let created: serde_json::Value =
                serde_json::from_str(&server.request("POST", "/sessions", "")).unwrap();
            let id = created["id"].as_u64().unwrap();
            assert!(id <= session, "session {session} was skipped");
            if id == session {
                break;
            }
        }
        let body = format!(r#"{{"turn":0,"prompt":"{prompt}"}}"#);
        let started = server.request("POST", &format!("/sessions/{session}/turns"), &body);
        let started: serde_json::Value = serde_json::from_str(&started).expect(&started);
        let path = format!("/goals/{}", started["goal"]);
        wait_until("the reference turn", WAIT, || {
            let goal: serde_json::Value =
                serde_json::from_str(&server.request("GET", &path, "")).unwrap();
            (goal["status"] == "done").then(|| output(&goal))
        })
    }

    /// Start `rs3 acp` over every node in order, open a session and send one prompt; return the
    /// adapter, the session id and the text of the first chunk.
    fn start_turn(
        cluster: &Cluster,
        env: &[(&str, &str)],
        prompt: &str,
    ) -> (crate::acp::Acp, u64, String) {
        start_turn_on(
            cluster,
            &node_urls(cluster, 0..cluster.ports.len()),
            env,
            prompt,
        )
    }

    fn node_urls(cluster: &Cluster, nodes: std::ops::Range<usize>) -> Vec<String> {
        nodes
            .map(|n| format!("http://127.0.0.1:{}", cluster.ports[n]))
            .collect()
    }

    /// Like `start_turn`, with the adapter given only `urls`.
    fn start_turn_on(
        cluster: &Cluster,
        urls: &[String],
        env: &[(&str, &str)],
        prompt: &str,
    ) -> (crate::acp::Acp, u64, String) {
        cluster.leader();
        let mut acp = crate::acp::Acp::start_with(&cluster.dir.0, urls, env);
        let session = acp.new_session(1);
        assert_eq!(acp.advertised()["params"]["sessionId"], session.as_str());
        acp.request(
            2,
            "session/prompt",
            serde_json::json!({"sessionId": session, "prompt": [{"type": "text", "text": prompt}]}),
        );
        let first = acp.recv_within(WAIT).expect("no first chunk");
        assert_eq!(first["method"], "session/update", "{first}");
        let text = first["params"]["update"]["content"]["text"]
            .as_str()
            .unwrap();
        (acp, session.parse().unwrap(), text.to_owned())
    }

    /// Collect chunk texts until the response to the prompt, waiting up to `WAIT` for each line.
    fn finish_turn(acp: &mut crate::acp::Acp) -> (serde_json::Value, Vec<String>) {
        let mut chunks = Vec::new();
        loop {
            let message = acp.recv_within(WAIT).expect("no line from rs3 acp");
            if message["id"] == 2 {
                return (message, chunks);
            }
            chunks.push(
                message["params"]["update"]["content"]["text"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
        }
    }

    fn assert_one_turn_and_no_second_goal(cluster: &Cluster, live: &[usize], session: u64) {
        for &node in live {
            let (status, reply) = cluster.request_status(node, "GET", "/goals/2", "");
            assert_eq!(status, 404, "node {node}: {reply}");
            let body =
                cluster.request_when_available(node, "GET", &format!("/sessions/{session}"), "");
            let body: serde_json::Value = serde_json::from_str(&body).expect(&body);
            assert_eq!(
                body["turns"].as_array().unwrap().len(),
                1,
                "node {node}: {body}"
            );
        }
    }

    const SLOW: [(&str, &str); 1] = [("MODEL.TOKENS_PER_SECOND", "100")];

    #[test]
    fn acp_turn_survives_killing_the_owner_and_the_node_in_use() {
        let mut cluster = Cluster::start_with("cluster-acp-failover", 5, &SLOW);
        let (mut acp, session, first) = start_turn(&cluster, &[], "failover");
        let owner = cluster.goal(0, 1)["node"].as_u64().unwrap() as usize - 1;
        let second = if owner == 0 { 1 } else { owner };
        cluster.kill(0);
        cluster.kill(second);
        let (response, rest) = finish_turn(&mut acp);
        assert_eq!(response["result"]["stopReason"], "max_tokens", "{response}");
        let text = format!("{first}{}", rest.concat());
        assert!(
            text.len() > first.len(),
            "the kills came after the turn ended"
        );
        let live: Vec<usize> = (0..5).filter(|n| *n != 0 && *n != second).collect();
        let done = wait_until("the goal to be done", WAIT, || {
            let goal = cluster.goal(live[0], 1);
            (goal["status"] == "done").then_some(goal)
        });
        assert_eq!(text, output(&done));
        assert_eq!(text, single_node_turn(session, "failover"));
        let log = acp.stderr_text();
        assert!(
            log.lines()
                .any(|l| l.starts_with("GET ") || l.starts_with("POST ")),
            "no failed attempt in: {log}"
        );
        assert_one_turn_and_no_second_goal(&cluster, &live, session);
    }

    #[test]
    fn acp_run_script_turn_writes_once_across_an_owner_kill() {
        let mut cluster = Cluster::start("cluster-acp-script", 3);
        let source = r#"for (let i = 0; i < 5; i++) {
                Deno.core.ops.op_rs3_insert('{"s' + i + '":"' + i + '"}');
                const end = Date.now() + 1000;
                while (Date.now() < end) {}
            }
            String(Object.keys(JSON.parse(Deno.core.ops.op_rs3_list())).length)"#;
        cluster.request_when_available(
            0,
            "POST",
            "/insert",
            &serde_json::json!({ "job": source }).to_string(),
        );
        let mut acp = crate::acp::Acp::start_with(
            &cluster.dir.0,
            &node_urls(&cluster, 0..cluster.ports.len()),
            &[],
        );
        let session = acp.new_session(1);
        assert_eq!(acp.advertised()["params"]["sessionId"], session.as_str());
        acp.request(
            2,
            "session/prompt",
            serde_json::json!({"sessionId": session, "prompt": [{"type": "text", "text": "/run job"}]}),
        );
        let goal_id = wait_until("the session to name its goal", WAIT, || {
            let body =
                cluster.request_when_available(0, "GET", &format!("/sessions/{session}"), "");
            let body: serde_json::Value = serde_json::from_str(&body).ok()?;
            body["turns"].as_array()?.first()?["goal"].as_u64()
        });
        let victim = wait_until("the script to start writing", WAIT, || {
            let goal = cluster.goal(0, goal_id);
            let first = cluster.request_when_available(0, "GET", "/key/s0", "");
            (goal["status"] == "running" && first == r#""0""#).then(|| {
                goal["node"]
                    .as_u64()
                    .expect("a running goal names its node") as usize
                    - 1
            })
        });
        cluster.kill(victim);
        let live: Vec<usize> = (0..3).filter(|n| *n != victim).collect();
        let (response, chunks) = finish_turn(&mut acp);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        // `job` plus s0..s4, each written once.
        assert_eq!(chunks.concat(), "6");
        for &node in &live {
            for i in 0..5 {
                let got = cluster.request_when_available(node, "GET", &format!("/key/s{i}"), "");
                assert_eq!(got, format!(r#""{i}""#), "node {node}");
            }
            let body =
                cluster.request_when_available(node, "GET", &format!("/sessions/{session}"), "");
            let body: serde_json::Value = serde_json::from_str(&body).expect(&body);
            assert_eq!(body["turns"].as_array().unwrap().len(), 1, "{body}");
        }
    }

    #[test]
    fn acp_turn_waits_through_quorum_loss_and_completes_when_a_node_returns() {
        let mut cluster = Cluster::start_with("cluster-acp-quorum", 5, &SLOW);
        let (mut acp, session, first) = start_turn(&cluster, &[], "quorum");
        for node in 0..3 {
            cluster.kill(node);
        }
        let mut text = first;
        let quiet = Instant::now() + Duration::from_secs(5);
        while let Some(wait) = quiet.checked_duration_since(Instant::now()) {
            if let Some(message) = acp.recv_within(wait) {
                assert_ne!(message["id"], 2, "answered during the outage: {message}");
                text.push_str(
                    message["params"]["update"]["content"]["text"]
                        .as_str()
                        .unwrap(),
                );
            }
        }
        cluster.restart(0);
        let (response, rest) = finish_turn(&mut acp);
        assert_eq!(response["result"]["stopReason"], "max_tokens", "{response}");
        text.push_str(&rest.concat());
        assert_eq!(text, single_node_turn(session, "quorum"));
        assert_one_turn_and_no_second_goal(&cluster, &[0, 3, 4], session);
    }

    #[test]
    fn acp_turn_answers_cluster_unavailable_after_the_timeout_and_keeps_the_goal() {
        let mut cluster = Cluster::start_with("cluster-acp-timeout", 5, &SLOW);
        let (mut acp, session, first) =
            start_turn(&cluster, &[("ACP_UNAVAILABLE_TIMEOUT", "2")], "timeout");
        for node in 0..3 {
            cluster.kill(node);
        }
        let (response, rest) = finish_turn(&mut acp);
        let error = &response["error"];
        assert_eq!(error["code"], -32001, "{response}");
        assert_eq!(error["message"], "cluster_unavailable", "{response}");
        assert_eq!(error["data"]["error"], "cluster_unavailable", "{response}");
        let received = format!("{first}{}", rest.concat());
        for node in 0..3 {
            cluster.restart(node);
        }
        let done = wait_until("the goal to be done", WAIT, || {
            let goal = cluster.goal(3, 1);
            (goal["status"] == "done").then_some(goal)
        });
        assert!(
            output(&done).starts_with(&received),
            "{received:?} vs {done}"
        );
        assert_eq!(output(&done), single_node_turn(session, "timeout"));
        assert_one_turn_and_no_second_goal(&cluster, &[0, 1, 2, 3, 4], session);
        let mut fresh = crate::acp::Acp::start(&cluster.dir.0, &node_urls(&cluster, 0..5));
        let (response, updates) = fresh.load(1, &session.to_string(), WAIT);
        assert_eq!(response["result"], serde_json::Value::Null, "{response}");
        let replay = [
            ("user_message_chunk".to_owned(), "timeout".to_owned()),
            ("agent_message_chunk".to_owned(), output(&done)),
        ];
        assert_eq!(updates, replay);
        assert!(fresh.finish().success());
    }

    fn holds_fragment(cluster: &Cluster, node: usize, id: u64) -> bool {
        let path = format!("/goals/{id}/storage");
        let (status, reply) = cluster.request_status(node, "GET", &path, "");
        status == 200 && reply.contains("\"fragment\"")
    }

    #[test]
    fn acp_restart_mid_turn_loads_then_delivers_the_rest_of_the_turn_and_its_own() {
        let cluster = Cluster::start_with("cluster-acp-restart", 3, &SLOW);
        let (acp, session, _) = start_turn(&cluster, &[], "restart");
        drop(acp);
        let urls = node_urls(&cluster, 0..3);
        let mut fresh = crate::acp::Acp::start_with(&cluster.dir.0, &urls, &[]);
        let (response, updates) = fresh.load(1, &session.to_string(), WAIT);
        assert_eq!(response["result"], serde_json::Value::Null, "{response}");
        let replayed = updates[1].1.clone();
        let params = serde_json::json!({"sessionId": session.to_string(), "prompt": [{"type": "text", "text": "after"}]});
        fresh.request(2, "session/prompt", params);
        let (response, chunks) = finish_turn(&mut fresh);
        assert_eq!(response["result"]["stopReason"], "max_tokens", "{response}");
        let first = output(&cluster.goal(0, 1));
        let second = output(&cluster.goal(0, 2));
        assert_eq!(first, single_node_turn(session, "restart"));
        assert_eq!(
            format!("{replayed}{}", chunks.concat()),
            format!("{first}{second}")
        );
        for node in 0..3 {
            let (status, reply) = cluster.request_status(node, "GET", "/goals/3", "");
            assert_eq!(status, 404, "node {node}: {reply}");
            let path = format!("/sessions/{session}");
            let body = cluster.request_when_available(node, "GET", &path, "");
            let body: serde_json::Value = serde_json::from_str(&body).expect(&body);
            assert_eq!(body["turns"].as_array().unwrap().len(), 2, "{body}");
        }
        assert!(fresh.finish().success());
    }

    #[test]
    fn acp_load_replays_sealed_goals_byte_identical_with_two_nodes_killed() {
        let mut cluster = Cluster::start("cluster-acp-sealed", 5);
        cluster.leader();
        let urls = node_urls(&cluster, 0..5);
        let mut writer = crate::acp::Acp::start_with(&cluster.dir.0, &urls, &[]);
        let session = writer.new_session(1);
        let mut texts = Vec::new();
        for (id, prompt) in [(2, "one"), (3, "two")] {
            let (response, chunks) = writer.prompt(id, &session, &[prompt]);
            assert_eq!(response["result"]["stopReason"], "max_tokens", "{response}");
            texts.push(chunks.concat());
        }
        assert!(writer.finish().success());
        for node in 0..5 {
            wait_until("both goals sealed", WAIT, || {
                (holds_fragment(&cluster, node, 1) && holds_fragment(&cluster, node, 2))
                    .then_some(())
            });
        }
        cluster.kill(0);
        cluster.kill(2);
        let mut acp = crate::acp::Acp::start(&cluster.dir.0, &urls);
        let (response, updates) = acp.load(1, &session, WAIT);
        assert_eq!(response["result"], serde_json::Value::Null, "{response}");
        let user = |t: &str| ("user_message_chunk".to_owned(), t.to_owned());
        let agent = |t: &String| ("agent_message_chunk".to_owned(), t.clone());
        let expected = [user("one"), agent(&texts[0]), user("two"), agent(&texts[1])];
        assert_eq!(updates, expected);
        assert!(acp.finish().success());
    }

    #[test]
    fn acp_cancel_mid_turn_answers_cancelled_and_the_goal_is_cancelled_on_every_node() {
        let cluster = Cluster::start_with("cluster-acp-cancel", 5, &SLOW);
        let (mut acp, session, first) = start_turn(&cluster, &[], "cancel");
        acp.cancel(&session.to_string());
        let (response, rest) = finish_turn(&mut acp);
        assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
        for node in 0..5 {
            let goal = wait_until("the goal to be cancelled", WAIT, || {
                let goal = cluster.goal(node, 1);
                (goal["status"] == "cancelled").then_some(goal)
            });
            assert_eq!(format!("{first}{}", rest.concat()), output(&goal));
        }
    }

    #[test]
    fn acp_cancel_with_quorum_lost_answers_cluster_unavailable_and_leaves_the_goal() {
        let mut cluster = Cluster::start_with("cluster-acp-cancel-quorum", 5, &SLOW);
        // Only the nodes about to die: a DELETE that reached a surviving node could commit
        // later, so this way the cancel can never reach the cluster.
        let urls = node_urls(&cluster, 0..3);
        let (mut acp, session, first) = start_turn_on(
            &cluster,
            &urls,
            &[("ACP_UNAVAILABLE_TIMEOUT", "2")],
            "cancel",
        );
        for node in 0..3 {
            cluster.kill(node);
        }
        acp.cancel(&session.to_string());
        let (response, rest) = finish_turn(&mut acp);
        assert_eq!(response["error"]["code"], -32001, "{response}");
        let received = format!("{first}{}", rest.concat());
        cluster.restart(0);
        let (response, later) = acp.call(
            3,
            "session/prompt",
            serde_json::json!({"sessionId": session.to_string(), "prompt": [{"type": "text", "text": "again"}]}),
        );
        assert_eq!(response["result"]["stopReason"], "max_tokens", "{response}");
        let done = wait_until("the goal to be done", WAIT, || {
            let goal = cluster.goal(3, 1);
            (goal["status"] == "done").then_some(goal)
        });
        let later: String = later
            .iter()
            .map(|u| u["params"]["update"]["content"]["text"].as_str().unwrap())
            .collect();
        assert!(
            format!("{received}{later}").starts_with(&output(&done)),
            "{received:?} {later:?} vs {done}"
        );
    }

    #[test]
    fn acp_cancel_during_a_pending_prompt_takes_effect_when_quorum_returns() {
        let mut cluster = Cluster::start_with("cluster-acp-cancel-pending", 5, &SLOW);
        cluster.leader();
        let urls = node_urls(&cluster, 0..5);
        let mut acp = crate::acp::Acp::start(&cluster.dir.0, &urls);
        let session = acp.new_session(1);
        for node in 0..3 {
            cluster.kill(node);
        }
        acp.request(
            2,
            "session/prompt",
            serde_json::json!({"sessionId": session, "prompt": [{"type": "text", "text": "pending"}]}),
        );
        acp.cancel(&session);
        if let Some(message) = acp.recv_within(Duration::from_secs(5)) {
            panic!("answered during the outage: {message}");
        }
        cluster.restart(0);
        let (response, _) = finish_turn(&mut acp);
        assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
        for node in [0, 3, 4] {
            wait_until("goal 1 to be cancelled", WAIT, || {
                (cluster.goal(node, 1)["status"] == "cancelled").then_some(())
            });
        }
    }

    #[test]
    fn acp_cancel_after_the_goal_ended_still_answers_cancelled() {
        let mut cluster = Cluster::start_with("cluster-acp-cancel-ended", 5, &SLOW);
        let urls = node_urls(&cluster, 0..1);
        let (mut acp, session, _) = start_turn_on(&cluster, &urls, &[], "ended");
        cluster.kill(0);
        wait_until("the goal to be done", WAIT, || {
            (cluster.goal(1, 1)["status"] == "done").then_some(())
        });
        acp.cancel(&session.to_string());
        cluster.restart(0);
        let (response, _) = finish_turn(&mut acp);
        assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
        assert_eq!(cluster.goal(1, 1)["status"], "done");
    }
}

#[cfg(test)]
mod cluster_model {
    use std::path::Path;
    use std::time::Duration;

    use crate::cluster::{Cluster, wait_until};
    use crate::common::{Server, TempDir};
    use crate::model::model_files;

    /// Real-model tests run one at a time: concurrent model loads starve or kill nodes.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
    }

    const BUDGET: u64 = 48;
    const PROMPT: &str =
        "List twenty different animals, one per line, with a short description of each. /no_think";

    fn model_env<'a>(gguf: &'a Path, tokenizer: &'a Path) -> [(&'a str, &'a str); 3] {
        [
            ("MODEL.PATH", gguf.to_str().expect("utf-8 path")),
            ("MODEL.TOKENIZER", tokenizer.to_str().expect("utf-8 path")),
            ("MODEL.THREADS", "1"),
        ]
    }

    fn body() -> String {
        serde_json::json!({"priority": 0, "budget": BUDGET, "seed": 11, "prompt": PROMPT})
            .to_string()
    }

    /// The finished goal of one uninterrupted single-node run.
    fn reference() -> serde_json::Value {
        let (gguf, tokenizer) = model_files();
        let dir = TempDir::new("cluster-model-ref");
        let env = model_env(&gguf, &tokenizer);
        let server = Server::start(&dir.0, Some(&dir.0.join("rs3.redb")), &env);
        let (status, text) = server.request_status("POST", "/goals", &body());
        assert_eq!(status, 201, "{text}");
        wait_until("the reference goal", Duration::from_secs(180), || {
            let got: serde_json::Value =
                serde_json::from_str(&server.request("GET", "/goals/1", "")).unwrap();
            (got["status"] == "done").then_some(got)
        })
    }

    #[test]
    fn mismatch_node_declines_and_goal_finishes_elsewhere() {
        let _one = exclusive();
        let reference = reference();
        assert_eq!(reference["reason"], "budget", "{reference}");
        let (gguf, tokenizer) = model_files();
        let dir = TempDir::new("cluster-model-files");
        // A copy of the model with its last byte flipped: a different model file.
        let other = dir.0.join("other.gguf");
        let mut bytes = std::fs::read(&gguf).unwrap();
        *bytes.last_mut().unwrap() ^= 0xff;
        std::fs::write(&other, bytes).unwrap();

        let env = model_env(&gguf, &tokenizer);
        let own = vec![("MODEL.PATH".to_owned(), other.to_str().unwrap().to_owned())];
        let cluster = Cluster::start_with_nodes("cluster-model", 3, &env, vec![(0, own)]);

        let id = cluster.create_goal(1, &body());
        let got = wait_until("the goal to finish", Duration::from_secs(180), || {
            let got = cluster.goal(1, id);
            (got["status"] == "done").then_some(got)
        });
        assert_eq!(got["output"], reference["output"]);
        assert_eq!(got["reason"], "budget");
        assert_ne!(
            got["node"], 1,
            "the node on another model finished it: {got}"
        );
        let generated: serde_json::Value =
            serde_json::from_str(&cluster.request(0, "GET", "/goals/generated", "")).unwrap();
        assert_eq!(generated["generated"].as_u64(), Some(0), "{generated}");
    }

    /// Most tokens one uncommitted batch can hold: a survivor may sample that many beyond the
    /// positions nobody committed.
    const BATCH_SLACK: u64 = 8;

    /// Goal `id` as node `node` shows it, or `None` while the node does not answer yet.
    fn try_goal(cluster: &Cluster, node: usize, id: u64) -> Option<serde_json::Value> {
        let (status, reply) = cluster.request_status(node, "GET", &format!("/goals/{id}"), "");
        (status == 200).then(|| serde_json::from_str(&reply).ok())?
    }

    #[test]
    fn failover_after_killing_the_owner_and_another_node_samples_no_committed_position_again() {
        let _one = exclusive();
        let reference = reference();
        let (gguf, tokenizer) = model_files();
        let mut cluster =
            Cluster::start_with("cluster-model-failover", 5, &model_env(&gguf, &tokenizer));
        let id = cluster.create_goal(0, &body());
        let wait = Duration::from_secs(240);
        let owner = wait_until("output on a running goal", wait, || {
            let got = try_goal(&cluster, 0, id)?;
            let started = got["status"] == "running" && !got["output"].as_str()?.is_empty();
            started.then(|| got["node"].as_u64().map(|n| n as usize - 1))?
        });
        let leader = cluster.leader();
        let other = if leader != owner {
            leader
        } else {
            (owner + 1) % 5
        };
        cluster.kill(owner);
        cluster.kill(other);

        let db = cluster.dir.0.join(format!("node{}.db", owner + 1));
        let committed = {
            let journal = rs3::journal::Journal::new(rs3::storage::open(&db).unwrap()).unwrap();
            let state = journal.restore(id).unwrap().expect("goal stream exists");
            assert!(
                state.ended.is_none(),
                "the kill landed after the goal ended"
            );
            state.tokens.len() as u64
        };
        assert!(
            0 < committed && committed < BUDGET,
            "committed {committed} of {BUDGET}"
        );

        let survivors: Vec<usize> = (0..5).filter(|i| *i != owner && *i != other).collect();
        let done = wait_until("the goal to finish on a survivor", wait, || {
            let got = try_goal(&cluster, survivors[0], id)?;
            (got["status"] == "done").then_some(got)
        });
        assert_eq!(done["output"], reference["output"], "C={committed}");
        assert_eq!(done["reason"], reference["reason"]);
        let on = done["node"].as_u64().unwrap() as usize - 1;
        assert!(survivors.contains(&on), "finished on killed node: {done}");

        let sampled: u64 = survivors
            .iter()
            .map(|&node| {
                let reply = cluster.request(node, "GET", "/goals/generated", "");
                let got: serde_json::Value = serde_json::from_str(&reply).unwrap();
                got["generated"].as_u64().unwrap()
            })
            .sum();
        let uncommitted = BUDGET - committed;
        assert!(
            sampled <= uncommitted,
            "resampled: {sampled} > {uncommitted}, C={committed}"
        );
        assert!(
            sampled + BATCH_SLACK >= uncommitted,
            "a survivor sampled {sampled} of {uncommitted}, C={committed}"
        );

        for &node in &survivors {
            let path = format!("/goals/{id}/storage");
            wait_until(
                "a fragment on every survivor",
                Duration::from_secs(60),
                || {
                    let (status, reply) = cluster.request_status(node, "GET", &path, "");
                    (status == 200 && reply.contains("\"fragment\"")).then_some(())
                },
            );
        }
        for &node in &survivors {
            let got = wait_until("a survivor to answer", Duration::from_secs(60), || {
                try_goal(&cluster, node, id)
            });
            assert_eq!(got["output"], reference["output"], "node {node}");
        }
    }
}
