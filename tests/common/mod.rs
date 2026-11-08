//! Helpers shared by the integration tests that run the real binary.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub fn rs3(port: u16, dir: &Path, db: Option<&Path>) -> Command {
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

pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A server that has announced it is listening. It is killed with SIGKILL on drop.
pub struct Server {
    pub child: Child,
    pub port: u16,
}

impl Server {
    pub fn start(dir: &Path, db: Option<&Path>, env: &[(&str, &str)]) -> Self {
        Self::start_on(free_port(), dir, db, env)
    }

    pub fn start_on(port: u16, dir: &Path, db: Option<&Path>, env: &[(&str, &str)]) -> Self {
        let mut command = rs3(port, dir, db);
        // A file, not a pipe nobody reads: a burst of refused requests would fill a pipe and
        // block the node, and the file shows why a test timed out.
        let log = dir.join(format!("rs3-{port}.log"));
        let stderr = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .unwrap();
        command.envs(env.iter().copied()).stderr(stderr);
        Self::spawn(command, port, Duration::from_secs(30))
    }

    /// Spawn `command` and wait up to `deadline` for the startup banner; a node that never
    /// prints it is killed and the test fails.
    pub fn spawn(mut command: Command, port: u16, deadline: Duration) -> Self {
        let mut child = command.stdout(Stdio::piped()).spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sent, banner) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let _ = sent.send(line);
            // Keep draining so a chatty node never blocks on a full pipe.
            let _ = std::io::copy(&mut reader, &mut std::io::sink());
        });
        let Ok(banner) = banner.recv_timeout(deadline) else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("no startup banner within {deadline:?}");
        };
        assert!(banner.starts_with("Starting Http server"), "got {banner:?}");
        Self { child, port }
    }

    /// The response body of one HTTP request.
    pub fn request(&self, method: &str, path: &str, body: &str) -> String {
        self.request_status(method, path, body).1
    }

    /// The status code and response body of one HTTP request.
    pub fn request_status(&self, method: &str, path: &str, body: &str) -> (u16, String) {
        let mut stream = TcpStream::connect_timeout(
            &(std::net::Ipv4Addr::LOCALHOST, self.port).into(),
            Duration::from_secs(15),
        )
        .unwrap();
        let bound = Some(Duration::from_secs(15));
        stream.set_read_timeout(bound).unwrap();
        stream.set_write_timeout(bound).unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: rs3\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        (status, body.to_owned())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
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
