//! The Raft RPC routes: a peer's `Network` posts here. Each call runs on the node's own runtime.
use std::path::PathBuf;

use actix_web::{HttpResponse, get, post, web};
use openraft::raft::{AppendEntriesRequest, InstallSnapshotRequest, VoteRequest};
use serde::Serialize;

use crate::raft::{ClusterError, Node, TypeConfig};
use crate::store::Command;

/// Register the routes; they need the node and the directory of its fragment files.
pub fn configure(
    node: web::Data<Node>,
    fragments: PathBuf,
) -> impl Fn(&mut web::ServiceConfig) + Clone + Send + 'static {
    move |cfg: &mut web::ServiceConfig| {
        cfg.app_data(node.clone())
            .app_data(web::Data::new(Fragments(fragments.clone())))
            .service(fragment)
            .service(append)
            .service(vote)
            .service(snapshot)
            .service(forward)
            .service(read_index);
    }
}

/// Run `call` on the node's runtime and answer with its `Result` as JSON.
async fn run<T: Serialize + Send + 'static>(
    node: &Node,
    call: impl Future<Output = T> + Send + 'static,
) -> HttpResponse {
    match node.handle().spawn(call).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

pub(crate) struct Fragments(pub(crate) PathBuf);

/// The raw bytes of fragment file `stream` in `dir`, or 404. The file name is built from the
/// parsed id, never from a raw path segment.
pub(crate) async fn read_fragment(dir: PathBuf, stream: u64) -> HttpResponse {
    match web::block(move || std::fs::read(dir.join(stream.to_string()))).await {
        Ok(Ok(bytes)) => HttpResponse::Ok().body(bytes),
        _ => HttpResponse::NotFound().finish(),
    }
}

/// `GET /raft/fragment/{stream}` serves this node's fragment of a sealed stream.
#[get("/raft/fragment/{stream}")]
async fn fragment(dir: web::Data<Fragments>, stream: web::Path<u64>) -> HttpResponse {
    read_fragment(dir.0.clone(), *stream).await
}

#[post("/raft/append")]
async fn append(
    node: web::Data<Node>,
    req: web::Json<AppendEntriesRequest<TypeConfig>>,
) -> HttpResponse {
    let raft = node.raft().clone();
    run(
        &node,
        async move { raft.append_entries(req.into_inner()).await },
    )
    .await
}

#[post("/raft/vote")]
async fn vote(node: web::Data<Node>, req: web::Json<VoteRequest<u64>>) -> HttpResponse {
    let raft = node.raft().clone();
    run(&node, async move { raft.vote(req.into_inner()).await }).await
}

#[post("/raft/snapshot")]
async fn snapshot(
    node: web::Data<Node>,
    req: web::Json<InstallSnapshotRequest<TypeConfig>>,
) -> HttpResponse {
    let raft = node.raft().clone();
    run(&node, async move {
        raft.install_snapshot(req.into_inner()).await
    })
    .await
}

/// Answer a forwarded request: a refusal is a 503, so a forward is never forwarded again.
pub(crate) fn answer<T: Serialize>(result: Result<T, ClusterError>) -> HttpResponse {
    match result {
        Ok(done) => HttpResponse::Ok().json(done),
        Err(e) => HttpResponse::ServiceUnavailable().body(e.to_string()),
    }
}

#[post("/raft/forward")]
async fn forward(node: web::Data<Node>, cmd: web::Json<Command>) -> HttpResponse {
    answer(node.write(cmd.into_inner()).await)
}

#[post("/raft/read_index")]
async fn read_index(node: web::Data<Node>) -> HttpResponse {
    answer(node.read_index().await)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use actix_web::{App, http::StatusCode, test as atest};

    use super::*;
    use crate::raft::testing::{Cluster, WAIT};
    use crate::store::{Command, Store};

    #[actix_web::test]
    async fn a_malformed_vote_is_a_client_error() {
        let dir = crate::storage::TempDir::new();
        let db = crate::storage::open(&dir.path().join("db")).unwrap();
        let members = BTreeMap::from([(1, "127.0.0.1:1".to_owned())]);
        let node = Node::start(
            1,
            &members,
            db.clone(),
            Store::durable(db).unwrap(),
            crate::raft::testing::SNAPSHOT_EVERY,
        )
        .await
        .unwrap();
        let app = atest::init_service(
            App::new().configure(configure(web::Data::new(node), PathBuf::new())),
        )
        .await;
        let req = atest::TestRequest::post()
            .uri("/raft/vote")
            .insert_header(("content-type", "application/json"))
            .set_payload("{not json")
            .to_request();
        assert_eq!(
            atest::call_service(&app, req).await.status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[actix_web::test]
    async fn a_snapshot_offered_to_a_node_is_answered_with_its_result() {
        let dir = crate::storage::TempDir::new();
        let db = crate::storage::open(&dir.path().join("db")).unwrap();
        let members = BTreeMap::from([(1, "127.0.0.1:1".to_owned())]);
        let node = Node::start(
            1,
            &members,
            db.clone(),
            Store::durable(db).unwrap(),
            crate::raft::testing::SNAPSHOT_EVERY,
        )
        .await
        .unwrap();
        let app = atest::init_service(
            App::new().configure(configure(web::Data::new(node), PathBuf::new())),
        )
        .await;
        let req = atest::TestRequest::post()
            .uri("/raft/snapshot")
            .set_json(InstallSnapshotRequest::<TypeConfig> {
                vote: openraft::Vote::new(1, 1),
                meta: openraft::SnapshotMeta::default(),
                offset: 0,
                data: Vec::new(),
                done: false,
            })
            .to_request();
        let body: serde_json::Value = atest::call_and_read_body_json(&app, req).await;
        assert!(
            body.get("Ok").is_some() || body.get("Err").is_some(),
            "{body}"
        );
    }

    #[actix_web::test]
    async fn a_write_proposed_on_one_node_reaches_every_node_of_a_three_node_cluster() {
        let cluster = Cluster::start(3, 3).await;
        cluster.follower().await;
        let cmd = Command::Put {
            entries: vec![("k".to_owned(), "v".to_owned())],
        };
        let proposer = cluster.follower().await;
        cluster.nodes[proposer].propose(cmd).await.unwrap().unwrap();
        assert_eq!(
            cluster.stores[proposer].get("k").unwrap(),
            "v",
            "the proposer reads its own write at once"
        );
        let index = cluster.nodes[proposer]
            .raft()
            .metrics()
            .borrow()
            .last_applied;

        for (node, store) in cluster.nodes.iter().zip(&cluster.stores) {
            node.raft()
                .wait(WAIT)
                .applied_index_at_least(index.map(|id| id.index), "the write applied")
                .await
                .unwrap();
            assert_eq!(store.get("k").unwrap(), "v");
        }
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn the_read_index_covers_every_committed_write() {
        let cluster = Cluster::start(1, 1).await;
        cluster.follower().await;
        let node = &cluster.nodes[0];
        let cmd = Command::Put {
            entries: vec![("k".to_owned(), "v".to_owned())],
        };
        let written = node.write(cmd).await.unwrap();
        assert!(node.read_index().await.unwrap() >= Some(written.log_index));
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_follower_reads_its_own_writes_at_once() {
        let cluster = Cluster::start(3, 3).await;
        let follower = cluster.follower().await;
        for i in 0..50 {
            let cmd = Command::Put {
                entries: vec![(format!("k{i}"), i.to_string())],
            };
            cluster.nodes[follower].propose(cmd).await.unwrap().unwrap();
            assert_eq!(
                cluster.stores[follower].get(&format!("k{i}")).unwrap(),
                i.to_string()
            );
        }
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_follower_retries_a_read_the_leader_it_asked_refuses_as_not_the_leader() {
        let cluster = Cluster::start(3, 3).await;
        let follower = cluster.follower().await;
        cluster
            .refuse
            .0
            .store(3, std::sync::atomic::Ordering::SeqCst);
        let started = std::time::Instant::now();
        let got = cluster.nodes[follower].read_barrier().await;
        assert!(got.is_ok(), "{got:?}");
        assert_eq!(
            cluster.refuse.0.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_follower_answers_no_quorum_when_its_leader_has_lost_the_majority() {
        let cluster = Cluster::start(5, 5).await;
        cluster.follower().await;
        let leader = cluster.nodes[0].raft().metrics().borrow().current_leader;
        let mut others = (0..5).filter(|i| Some(*i as u64 + 1) != leader);
        let asker = others.next().unwrap();
        for i in others.take(3) {
            cluster.nodes[i].raft().shutdown().await.unwrap();
        }
        let started = std::time::Instant::now();
        let got = cluster.nodes[asker].read_barrier().await;
        assert!(matches!(got, Err(ClusterError::NoQuorum)), "{got:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(6));
        cluster.stop().await;
    }
}
