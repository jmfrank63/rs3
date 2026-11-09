//! A cluster of real rs3 processes for the tests that need several nodes.

use std::net::TcpListener;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::common::{Server, TempDir};

const WAIT: Duration = Duration::from_secs(60);

/// Clusters run one at a time: several 5-node clusters at once starve each other.
static SERIAL: Mutex<()> = Mutex::new(());

/// N real nodes forming one Raft cluster, each with its own database file.
pub struct Cluster {
    /// `None` while a node is killed.
    nodes: Vec<Option<Server>>,
    pub ports: Vec<u16>,
    peers: String,
    /// The directory of every node's database and fragment files.
    pub dir: TempDir,
    env: Vec<(String, String)>,
    /// Nodes paused with SIGSTOP; `leader` skips them.
    stopped: Vec<usize>,
    /// Held for the cluster's lifetime; dropped last, after the nodes stop.
    _serial: MutexGuard<'static, ()>,
}

impl Cluster {
    pub fn start(name: &str, n: usize) -> Self {
        Self::start_with(name, n, &[])
    }

    /// Like `start`, with extra environment variables for every node.
    pub fn start_with(name: &str, n: usize, env: &[(&str, &str)]) -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new(name);
        let listeners: Vec<TcpListener> = (0..n)
            .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
            .collect();
        let ports: Vec<u16> = listeners
            .iter()
            .map(|l| l.local_addr().unwrap().port())
            .collect();
        drop(listeners);
        let peers = ports
            .iter()
            .zip(1..)
            .map(|(port, id)| format!("{id}=127.0.0.1:{port}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut cluster = Self {
            nodes: Vec::new(),
            ports,
            peers,
            dir,
            stopped: Vec::new(),
            _serial: serial,
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        for i in 0..cluster.ports.len() {
            let node = cluster.spawn(i);
            cluster.nodes.push(Some(node));
        }
        cluster
    }

    fn spawn(&self, i: usize) -> Server {
        let db = self.dir.0.join(format!("node{}.db", i + 1));
        let id = (i + 1).to_string();
        let mut env = vec![
            ("CLUSTER.NODE_ID", id.as_str()),
            ("CLUSTER.PEERS", self.peers.as_str()),
            // Small, so a test reaches a snapshot and a log purge with few writes.
            ("CLUSTER.SNAPSHOT_EVERY", "50"),
            // Short, so a test sees a shrink without a long wait.
            ("CLUSTER.SHRINK_AFTER", "5"),
        ];
        env.extend(self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        Server::start_on(self.ports[i], &self.dir.0, Some(&db), &env)
    }

    fn node(&self, i: usize) -> &Server {
        self.nodes[i].as_ref().expect("node is killed")
    }

    /// SIGKILL node `i` and wait for it to exit.
    pub fn kill(&mut self, i: usize) {
        self.nodes[i] = None;
    }

    fn signal(&self, i: usize, signal: &str) {
        let pid = self.node(i).child.id().to_string();
        let status = std::process::Command::new("kill")
            .args([signal, &pid])
            .status()
            .unwrap();
        assert!(status.success(), "kill {signal} {pid}");
    }

    /// Pause node `i` with SIGSTOP. Requests to it hang until `cont`.
    pub fn stop(&mut self, i: usize) {
        self.signal(i, "-STOP");
        self.stopped.push(i);
    }

    /// Resume node `i` with SIGCONT.
    pub fn cont(&mut self, i: usize) {
        self.signal(i, "-CONT");
        self.stopped.retain(|s| *s != i);
    }

    /// Start node `i` again on its old port and database.
    pub fn restart(&mut self, i: usize) {
        assert!(self.nodes[i].is_none(), "node {i} is running");
        self.nodes[i] = Some(self.spawn(i));
    }

    /// The index of the node that currently leads, polled for up to 30 s.
    pub fn leader(&self) -> usize {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            for (i, node) in self.nodes.iter().enumerate() {
                let Some(node) = node.as_ref().filter(|_| !self.stopped.contains(&i)) else {
                    continue;
                };
                if node.request_status("POST", "/raft/read_index", "").0 == 200 {
                    return i;
                }
            }
            assert!(Instant::now() < deadline, "no leader within 30 s");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// The response body of one HTTP request to node `i`.
    pub fn request(&self, i: usize, method: &str, path: &str, body: &str) -> String {
        self.node(i).request(method, path, body)
    }

    /// The status code and response body of one HTTP request to node `i`.
    pub fn request_status(&self, i: usize, method: &str, path: &str, body: &str) -> (u16, String) {
        self.node(i).request_status(method, path, body)
    }

    /// Repeat a request to node `i` until the cluster answers (no leader yet is 503 "unavailable", no quorum is 503 "no_quorum").
    pub fn request_when_available(&self, i: usize, method: &str, path: &str, body: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let reply = self.request(i, method, path, body);
            if !reply.contains("unavailable") && !reply.contains("no_quorum") {
                return reply;
            }
            assert!(Instant::now() < deadline, "node {i} unavailable: {reply}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Create a goal through node `i` and return its id.
    pub fn create_goal(&self, i: usize, body: &str) -> u64 {
        let reply = self.request_when_available(i, "POST", "/goals", body);
        let created: serde_json::Value = serde_json::from_str(&reply).expect(&reply);
        created["id"].as_u64().expect(&reply)
    }

    /// Goal `id` as node `i` shows it.
    pub fn goal(&self, i: usize, id: u64) -> serde_json::Value {
        let reply = self.request_when_available(i, "GET", &format!("/goals/{id}"), "");
        serde_json::from_str(&reply).expect(&reply)
    }
}

/// The last lines of every node log of this test process, for a failing test's message.
fn node_log_tails() -> String {
    let suffix = format!("-{}", std::process::id());
    let mut logs: Vec<_> = std::fs::read_dir(std::env::temp_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|dir| {
            let name = dir.file_name().to_string_lossy().into_owned();
            name.starts_with("rs3-it-") && name.ends_with(&suffix)
        })
        .flat_map(|dir| {
            std::fs::read_dir(dir.path())
                .into_iter()
                .flatten()
                .flatten()
        })
        .map(|file| file.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "log"))
        .collect();
    logs.sort();
    let mut out = String::new();
    for log in logs {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let tail = &lines[lines.len().saturating_sub(40)..];
        out.push_str(&format!("\n--- {} ---\n{}", log.display(), tail.join("\n")));
    }
    out
}

/// Poll `f` every 50 ms until it returns `Some`; fail after `deadline`.
pub fn wait_until<T>(what: &str, deadline: Duration, mut f: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + deadline;
    loop {
        if let Some(found) = f() {
            return found;
        }
        assert!(
            Instant::now() < end,
            "timed out waiting for {what}{}",
            node_log_tails()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

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
