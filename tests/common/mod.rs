//! Helpers shared by the integration tests that run the real binary.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

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

/// A server that has announced it is listening. It is killed with SIGKILL on drop.
pub struct Server {
    pub child: Child,
    pub port: u16,
}

impl Server {
    pub fn start(dir: &Path, db: Option<&Path>, env: &[(&str, &str)]) -> Self {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut command = rs3(port, dir, db);
        command.envs(env.iter().copied());
        let mut child = command.spawn().unwrap();
        let mut banner = String::new();
        BufReader::new(child.stdout.as_mut().unwrap())
            .read_line(&mut banner)
            .unwrap();
        assert!(banner.starts_with("Starting Http server"), "got {banner:?}");
        Self { child, port }
    }

    /// The response body of one HTTP request.
    pub fn request(&self, method: &str, path: &str, body: &str) -> String {
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
