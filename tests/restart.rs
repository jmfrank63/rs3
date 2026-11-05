//! The real binary, seen as a client sees it: over HTTP, across process deaths.
#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Output, Stdio};

    fn rs3(port: u16, dir: &Path, db: Option<&Path>) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rs3"));
        command
            .env_remove("STORAGE.PATH")
            .env("SERVER.HOST", "127.0.0.1")
            .env("SERVER.PORT", port.to_string())
            .current_dir(dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(db) = db {
            command.env("STORAGE.PATH", db);
        }
        command
    }

    /// A server that has announced it is listening. It is killed with SIGKILL on drop.
    struct Server {
        child: Child,
        port: u16,
    }

    impl Server {
        fn start(dir: &Path, db: Option<&Path>) -> Self {
            let port = TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let mut child = rs3(port, dir, db).spawn().unwrap();
            let mut banner = String::new();
            BufReader::new(child.stdout.as_mut().unwrap())
                .read_line(&mut banner)
                .unwrap();
            assert!(banner.starts_with("Starting Http server"), "got {banner:?}");
            Self { child, port }
        }

        /// The response body of one HTTP request.
        fn request(&self, method: &str, path: &str, body: &str) -> String {
            let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
            write!(
                stream,
                "{method} {path} HTTP/1.1\r\nHost: rs3\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            let (_, body) = response.split_once("\r\n\r\n").unwrap();
            body.to_owned()
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("rs3-it-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn writes_and_deletes_survive_kill_9_with_storage_path() {
        let dir = TempDir::new("durable");
        let db = dir.0.join("rs3.db");
        {
            let server = Server::start(&dir.0, Some(&db));
            server.request("POST", "/insert", r#"{"kept":"1","gone":"2"}"#);
            server.request("DELETE", "/key/gone", "");
        }
        let server = Server::start(&dir.0, Some(&db));
        assert_eq!(
            server.request("GET", "/list", ""),
            r#""[{\"kept\" : \"1\"}]""#
        );
    }

    #[test]
    fn restart_empties_the_store_without_storage_path() {
        let dir = TempDir::new("memory");
        {
            let server = Server::start(&dir.0, None);
            server.request("POST", "/insert", r#"{"kept":"1"}"#);
            assert_eq!(server.request("GET", "/key/kept", ""), r#""1""#);
        }
        let server = Server::start(&dir.0, None);
        assert_eq!(server.request("GET", "/list", ""), r#""[]""#);
    }

    #[test]
    fn sigterm_shuts_down_gracefully_with_exit_0() {
        let dir = TempDir::new("sigterm");
        let mut server = Server::start(&dir.0, None);
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
        let server = Server::start(&dir.0, None);
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
        let server = Server::start(&dir.0, Some(&db));
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

        let memory = Server::start(&dir.0, None);
        let body = memory.request("GET", "/goals/1", "");
        assert!(body.contains("goals_unavailable"), "{body}");
    }
}
