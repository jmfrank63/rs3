//! The Raft network: JSON over HTTP POST to a peer's `/raft/*` routes.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use openraft::BasicNode;
use openraft::error::Unreachable;
use openraft::error::{InstallSnapshotError, NetworkError, RPCError, RaftError, RemoteError};
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{AppendEntriesRequest, AppendEntriesResponse};
use openraft::raft::{InstallSnapshotRequest, InstallSnapshotResponse};
use openraft::raft::{VoteRequest, VoteResponse};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::TypeConfig;

type Rpc<T, E = RaftError<u64>> = Result<T, RPCError<u64, BasicNode, E>>;

/// How long a call to a peer may take.
pub const RPC_TIMEOUT: Duration = Duration::from_secs(2);

/// When each peer last answered a call.
pub type Heard = Arc<Mutex<HashMap<u64, Instant>>>;

/// Builds one client per peer; every client shares one connection pool and one `Heard` map.
#[derive(Clone)]
pub struct Network {
    pub client: reqwest::Client,
    pub heard: Heard,
}

impl Network {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(300))
            .timeout(RPC_TIMEOUT)
            .build()?;
        Ok(Self {
            client,
            heard: Heard::default(),
        })
    }
}

/// The bytes of fragment file `stream` held by the peer at `addr`, or `None` when it is down,
/// slow or has none.
pub async fn fetch_fragment(client: &reqwest::Client, addr: &str, stream: u64) -> Option<Vec<u8>> {
    let reply = client
        .get(format!("http://{addr}/raft/fragment/{stream}"))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    Some(reply.bytes().await.ok()?.to_vec())
}

/// A connection to one peer.
pub struct Peer {
    client: reqwest::Client,
    id: u64,
    addr: String,
    heard: Heard,
}

impl Peer {
    /// Post `req` to `path` and decode the peer's `Result`. A peer that cannot be
    /// reached is `Unreachable`; anything unreadable is a `Network` error; an
    /// error the peer's raft returned is a `RemoteError`.
    async fn call<Q, T, E>(
        &self,
        path: &str,
        req: &Q,
    ) -> Result<T, Box<RPCError<u64, BasicNode, E>>>
    where
        Q: Serialize,
        T: DeserializeOwned,
        E: DeserializeOwned + std::error::Error,
    {
        let url = format!("http://{}/raft/{path}", self.addr);
        let sent = self.client.post(url).json(req).send().await;
        let reply = match sent {
            Ok(r) => r.error_for_status(),
            Err(e) => Err(e),
        };
        let body = match reply {
            Ok(r) => r.json::<Result<T, E>>().await,
            Err(e) => Err(e),
        };
        if body.is_ok() {
            self.heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(self.id, Instant::now());
        }
        match body {
            Ok(Ok(t)) => Ok(t),
            Ok(Err(e)) => Err(Box::new(RPCError::RemoteError(RemoteError::new_with_node(
                self.id,
                BasicNode::new(self.addr.clone()),
                e,
            )))),
            Err(e) if e.is_connect() || e.is_timeout() => {
                Err(Box::new(RPCError::Unreachable(Unreachable::new(&e))))
            }
            Err(e) => Err(Box::new(RPCError::Network(NetworkError::new(&e)))),
        }
    }
}

impl RaftNetworkFactory<TypeConfig> for Network {
    type Network = Peer;

    async fn new_client(&mut self, target: u64, node: &BasicNode) -> Peer {
        Peer {
            client: self.client.clone(),
            id: target,
            addr: node.addr.clone(),
            heard: self.heard.clone(),
        }
    }
}

impl RaftNetwork<TypeConfig> for Peer {
    async fn append_entries(
        &mut self,
        req: AppendEntriesRequest<TypeConfig>,
        _: RPCOption,
    ) -> Rpc<AppendEntriesResponse<u64>> {
        self.call("append", &req).await.map_err(|e| *e)
    }

    async fn install_snapshot(
        &mut self,
        req: InstallSnapshotRequest<TypeConfig>,
        _: RPCOption,
    ) -> Rpc<InstallSnapshotResponse<u64>, RaftError<u64, InstallSnapshotError>> {
        self.call("snapshot", &req).await.map_err(|e| *e)
    }

    async fn vote(&mut self, req: VoteRequest<u64>, _: RPCOption) -> Rpc<VoteResponse<u64>> {
        self.call("vote", &req).await.map_err(|e| *e)
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use actix_web::{App, HttpServer, post};
    use openraft::Vote;

    use super::*;

    #[post("/raft/vote")]
    async fn garbage() -> &'static str {
        "not json"
    }

    async fn vote_error(addr: String) -> Box<RPCError<u64, BasicNode, RaftError<u64>>> {
        let mut peer = Network::new()
            .unwrap()
            .new_client(2, &BasicNode::new(addr))
            .await;
        let option = RPCOption::new(Duration::from_secs(1));
        let sent = peer
            .vote(VoteRequest::new(Vote::new(1, 1), None), option)
            .await;
        Box::new(sent.unwrap_err())
    }

    #[actix_web::test]
    async fn a_peer_nobody_listens_on_is_unreachable() {
        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let err = *vote_error(closed.to_string()).await;
        assert!(matches!(err, RPCError::Unreachable(_)), "{err}");
    }

    #[actix_web::test]
    async fn a_peer_that_answers_garbage_is_a_network_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = HttpServer::new(|| App::new().service(garbage))
            .workers(1)
            .listen(listener)
            .unwrap()
            .run();
        let handle = server.handle();
        actix_web::rt::spawn(server);
        let err = *vote_error(addr).await;
        handle.stop(false).await;
        assert!(matches!(err, RPCError::Network(_)), "{err}");
    }
}
