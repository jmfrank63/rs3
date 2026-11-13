//! Drives a real `rs3 acp` child over newline-delimited JSON-RPC.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(10);

pub struct Acp {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    seen: Vec<String>,
    commands: Vec<Value>,
    stderr: PathBuf,
}

impl Acp {
    /// Spawn `rs3 acp <urls>`; its stderr goes to a file in `dir`.
    pub fn start(dir: &std::path::Path, urls: &[String]) -> Self {
        Self::start_with(dir, urls, &[])
    }

    /// Like `start`, with extra environment variables for the adapter.
    pub fn start_with(dir: &std::path::Path, urls: &[String], env: &[(&str, &str)]) -> Self {
        let stderr = dir.join(format!("acp-{}.log", std::process::id()));
        let log = std::fs::File::create(&stderr).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_rs3"))
            .arg("acp")
            .args(urls)
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log)
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (sent, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sent.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            seen: Vec::new(),
            commands: Vec::new(),
            stderr,
        }
    }

    pub fn send(&mut self, line: &str) {
        self.send_bytes(line.as_bytes());
    }

    pub fn send_bytes(&mut self, bytes: &[u8]) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(bytes).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    /// Everything the child wrote to stderr so far.
    pub fn stderr_text(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }

    /// Stdout lines up to its close; call after the child has exited.
    pub fn rest_of_stdout(&mut self) -> Vec<String> {
        self.lines.try_iter().collect()
    }

    pub fn request(&mut self, id: u64, method: &str, params: Value) {
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.send(&request.to_string());
    }

    /// Send the `session/cancel` notification, which has no id.
    pub fn cancel(&mut self, session: &str) {
        let note =
            json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}});
        self.send(&note.to_string());
    }

    /// The next stdout line, which must be a JSON-RPC 2.0 object.
    pub fn recv(&mut self) -> Value {
        self.recv_within(DEADLINE)
            .unwrap_or_else(|| self.fail("no line from rs3 acp"))
    }

    /// The next stdout line, or None when none arrives within `wait`. An
    /// `available_commands_update` is kept for `advertised` and not returned.
    pub fn recv_within(&mut self, wait: Duration) -> Option<Value> {
        let end = Instant::now() + wait;
        loop {
            let message = self.recv_raw(end)?;
            if is_advert(&message) {
                self.commands.push(message);
            } else {
                return Some(message);
            }
        }
    }

    /// The next stored `available_commands_update`, waiting up to DEADLINE for one.
    pub fn advertised(&mut self) -> Value {
        let end = Instant::now() + DEADLINE;
        while self.commands.is_empty() {
            match self.recv_raw(end) {
                Some(message) if is_advert(&message) => self.commands.push(message),
                Some(message) => self.fail(&format!("expected an advertisement, got {message}")),
                None => self.fail("no available_commands_update"),
            }
        }
        self.commands.remove(0)
    }

    fn recv_raw(&mut self, end: Instant) -> Option<Value> {
        let left = end.saturating_duration_since(Instant::now());
        let line = match self.lines.recv_timeout(left) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => return None,
            Err(RecvTimeoutError::Disconnected) => self.fail("rs3 acp closed stdout"),
        };
        self.seen.push(line.clone());
        let message: Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| self.fail(&format!("stdout line is not JSON ({e}): {line}")));
        assert_eq!(message["jsonrpc"], "2.0", "not JSON-RPC 2.0: {line}");
        Some(message)
    }

    /// The response to request `id` and the update notifications that came before it.
    pub fn call(&mut self, id: u64, method: &str, params: Value) -> (Value, Vec<Value>) {
        self.request(id, method, params);
        self.read_until(id)
    }

    /// Read to the response to request `id`: it and the notifications that came before it.
    pub fn read_until(&mut self, id: u64) -> (Value, Vec<Value>) {
        let mut updates = Vec::new();
        let end = Instant::now() + 6 * DEADLINE;
        loop {
            if Instant::now() >= end {
                self.fail("no response to the request in time");
            }
            let message = self.recv();
            if message["id"] == id {
                return (message, updates);
            }
            updates.push(message);
        }
    }

    /// Open a session and return its id.
    pub fn new_session(&mut self, id: u64) -> String {
        let (response, _) = self.call(id, "session/new", json!({"cwd": "/", "mcpServers": []}));
        response["result"]["sessionId"].as_str().unwrap().to_owned()
    }

    /// Send `session/load` and read to its response, waiting up to `wait` for each line: the
    /// response and the replayed updates as (sessionUpdate kind, text).
    pub fn load(
        &mut self,
        id: u64,
        session: &str,
        wait: Duration,
    ) -> (Value, Vec<(String, String)>) {
        let params = json!({"sessionId": session, "cwd": "/", "mcpServers": []});
        self.request(id, "session/load", params);
        let mut updates = Vec::new();
        loop {
            let message = self
                .recv_within(wait)
                .unwrap_or_else(|| self.fail("no line from rs3 acp during session/load"));
            if message["id"] == id {
                return (message, updates);
            }
            let update = &message["params"]["update"];
            let kind = update["sessionUpdate"].as_str().unwrap().to_owned();
            updates.push((kind, update["content"]["text"].as_str().unwrap().to_owned()));
        }
    }

    /// Run a prompt of text blocks to the end: the response and the chunk texts.
    pub fn prompt(&mut self, id: u64, session: &str, texts: &[&str]) -> (Value, Vec<String>) {
        let (response, kinds) = self.prompt_kinds(id, session, texts);
        (response, kinds.into_iter().map(|(_, text)| text).collect())
    }

    /// Like `prompt`, but each chunk as (sessionUpdate kind, text).
    pub fn prompt_kinds(
        &mut self,
        id: u64,
        session: &str,
        texts: &[&str],
    ) -> (Value, Vec<(String, String)>) {
        let blocks: Vec<Value> = texts
            .iter()
            .map(|t| json!({"type": "text", "text": t}))
            .collect();
        let (response, updates) = self.call(
            id,
            "session/prompt",
            json!({"sessionId": session, "prompt": blocks}),
        );
        let kinds = updates
            .iter()
            .map(|u| {
                let update = &u["params"]["update"];
                let kind = update["sessionUpdate"].as_str().unwrap().to_owned();
                (kind, update["content"]["text"].as_str().unwrap().to_owned())
            })
            .collect();
        (response, kinds)
    }

    /// Close stdin and wait for the child to exit.
    pub fn finish(&mut self) -> ExitStatus {
        self.stdin = None;
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            if Instant::now() >= deadline {
                self.fail("rs3 acp did not exit after stdin closed");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn fail(&self, what: &str) -> ! {
        panic!(
            "{what}; lines seen: {:?}; stderr:\n{}",
            self.seen,
            self.stderr_text()
        );
    }
}

fn is_advert(message: &Value) -> bool {
    message["params"]["update"]["sessionUpdate"] == "available_commands_update"
}

impl Drop for Acp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
