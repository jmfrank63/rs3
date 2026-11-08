//! Three real rs3 processes form one Raft cluster that every node reads and writes through.

#[cfg(test)]
#[path = "common/cluster.rs"]
mod cluster;
#[cfg(test)]
mod common;

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::cluster::{Cluster, goal_body, output, single_node_output, wait_until};
    use crate::common::{Server, TempDir, free_port, rs3};

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
}
