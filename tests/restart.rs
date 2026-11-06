//! The real binary, seen as a client sees it: over HTTP, across process deaths.
#[cfg(test)]
mod common;

#[cfg(test)]
mod tests {
    use std::process::{Command, Output};

    use crate::common::{Server, TempDir, rs3};

    #[test]
    fn a_node_that_never_prints_its_banner_fails_the_start_and_leaves_no_child() {
        let dir = TempDir::new("silent");
        let pidfile = dir.0.join("pid");
        let mut silent = Command::new("sh");
        silent
            .arg("-c")
            .arg(format!("echo $$ > {}; exec sleep 60", pidfile.display()));
        let started = std::time::Instant::now();
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Server::spawn(silent, 0, std::time::Duration::from_secs(1));
        }));
        assert!(failed.is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        let pid = std::fs::read_to_string(&pidfile).unwrap();
        assert!(!std::path::Path::new("/proc").join(pid.trim()).exists());
    }

    #[test]
    fn writes_and_deletes_survive_kill_9_with_storage_path() {
        let dir = TempDir::new("durable");
        let db = dir.0.join("rs3.db");
        {
            let server = Server::start(&dir.0, Some(&db), &[]);
            server.request("POST", "/insert", r#"{"kept":"1","gone":"2"}"#);
            server.request("DELETE", "/key/gone", "");
        }
        let server = Server::start(&dir.0, Some(&db), &[]);
        assert_eq!(
            server.request("GET", "/list", ""),
            r#""[{\"kept\" : \"1\"}]""#
        );
    }

    #[test]
    fn restart_empties_the_store_without_storage_path() {
        let dir = TempDir::new("memory");
        {
            let server = Server::start(&dir.0, None, &[]);
            server.request("POST", "/insert", r#"{"kept":"1"}"#);
            assert_eq!(server.request("GET", "/key/kept", ""), r#""1""#);
        }
        let server = Server::start(&dir.0, None, &[]);
        assert_eq!(server.request("GET", "/list", ""), r#""[]""#);
    }

    #[test]
    fn sigterm_shuts_down_gracefully_with_exit_0() {
        let dir = TempDir::new("sigterm");
        let mut server = Server::start(&dir.0, None, &[]);
        // The banner comes just before the signal handlers are installed.
        server.request("GET", "/", "");
        let pid = server.child.id().to_string();
        let sent = Command::new("kill").args(["-TERM", &pid]).status().unwrap();
        assert!(sent.success());
        assert_eq!(server.child.wait().unwrap().code(), Some(0));
    }

    #[test]
    fn taken_port_stops_startup_with_one_error_line() {
        let dir = TempDir::new("taken");
        let server = Server::start(&dir.0, None, &[]);
        let Output { status, stderr, .. } = rs3(server.port, &dir.0, None).output().unwrap();
        assert_eq!(status.code(), Some(1));
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("error: cannot bind"), "{stderr}");
    }

    #[test]
    fn unusable_storage_path_stops_startup_with_one_error_line() {
        let dir = TempDir::new("badpath");
        let db = dir.0.join("missing").join("rs3.db");
        let Output { status, stderr, .. } = rs3(1, &dir.0, Some(&db)).output().unwrap();
        assert_eq!(status.code(), Some(1));
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(
            stderr.starts_with("error: cannot open storage: "),
            "{stderr}"
        );
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }

    #[test]
    fn a_goal_finishes_with_its_budget_and_is_unavailable_without_storage() {
        let dir = TempDir::new("goals");
        let db = dir.0.join("rs3.db");
        let server = Server::start(&dir.0, Some(&db), &[]);
        let created = server.request(
            "POST",
            "/goals",
            r#"{"priority":0,"budget":20,"seed":3,"prompt":"hi"}"#,
        );
        assert_eq!(created, r#"{"id":1}"#);
        let mut goal = serde_json::Value::Null;
        for _ in 0..400 {
            goal = serde_json::from_str(&server.request("GET", "/goals/1", "")).unwrap();
            if goal["status"] == "done" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert_eq!(goal["status"], "done", "{goal}");
        assert_eq!(
            goal["output"].as_str().unwrap().split_whitespace().count(),
            20
        );

        let memory = Server::start(&dir.0, None, &[]);
        let body = memory.request("GET", "/goals/1", "");
        assert!(body.contains("goals_unavailable"), "{body}");
    }
}
