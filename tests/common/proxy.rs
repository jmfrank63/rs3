//! A TCP proxy in front of one node that can lose the reply to a turn request.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

const IO_TIMEOUT: Duration = Duration::from_secs(10);

struct Shared {
    upstream: u16,
    drop_left: AtomicUsize,
    dropped: AtomicUsize,
    stop: AtomicBool,
}

pub struct Proxy {
    pub port: u16,
    shared: Arc<Shared>,
}

fn timeouts(stream: &TcpStream) {
    stream.set_read_timeout(Some(IO_TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(IO_TIMEOUT)).unwrap();
}

/// One HTTP request: headers, then Content-Length body bytes.
fn read_request(client: &mut TcpStream) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        let n = client.read(&mut buf).ok().filter(|n| *n > 0)?;
        data.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&data[..head_end]).to_lowercase();
    let length = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while data.len() < head_end + length {
        let n = client.read(&mut buf).ok().filter(|n| *n > 0)?;
        data.extend_from_slice(&buf[..n]);
    }
    Some(data)
}

fn serve(mut client: TcpStream, shared: &Shared) {
    timeouts(&client);
    let Some(request) = read_request(&mut client) else {
        return;
    };
    let text = String::from_utf8_lossy(&request).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let (first, rest) = head.split_once("\r\n").unwrap_or((head, ""));
    let path = first.split(' ').nth(1).unwrap_or_default();
    let Ok(mut upstream) = TcpStream::connect(("127.0.0.1", shared.upstream)) else {
        return;
    };
    timeouts(&upstream);
    let forwarded = format!("{first}\r\n{rest}\r\nConnection: close\r\n\r\n{body}");
    let mut reply = Vec::new();
    if upstream.write_all(forwarded.as_bytes()).is_err()
        || upstream.read_to_end(&mut reply).is_err()
    {
        return;
    }
    let lose = path.ends_with("/turns")
        && shared
            .drop_left
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
    if lose {
        shared.dropped.fetch_add(1, Ordering::SeqCst);
        return;
    }
    let _ = client.write_all(&reply);
}

impl Proxy {
    /// Listen on a free port and forward to the node on `upstream`.
    pub fn start(upstream: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let shared = Arc::new(Shared {
            upstream,
            drop_left: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
        });
        let accepting = Arc::clone(&shared);
        std::thread::spawn(move || {
            for client in listener.incoming().flatten() {
                if accepting.stop.load(Ordering::SeqCst) {
                    break;
                }
                let shared = Arc::clone(&accepting);
                std::thread::spawn(move || serve(client, &shared));
            }
        });
        Self { port, shared }
    }

    /// Lose the replies to the next `n` turn requests.
    pub fn drop_next(&self, n: usize) {
        self.shared.drop_left.store(n, Ordering::SeqCst);
    }

    /// How many turn replies were lost so far.
    pub fn dropped(&self) -> usize {
        self.shared.dropped.load(Ordering::SeqCst)
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}
