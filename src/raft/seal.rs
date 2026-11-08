//! Sealing an ended goal's stream: the leader proposes how it is split into fragments, and every
//! member stores its own fragment on disk before confirming it.
use std::path::{Path, PathBuf};

use super::{BoxError, Node};
use crate::journal::{Journal, SealRecord, rows_in};
use crate::seal;
use crate::store::{Command, Response};

/// The fragment files of the node whose database is at `db`.
pub fn fragment_dir(db: &Path) -> PathBuf {
    let mut dir = db.as_os_str().to_owned();
    dir.push(".fragments");
    dir.into()
}

/// Leader only: seal every ended stream that is not sealed yet over the current voters. Fewer
/// than three voters never seal. A lost voter is kept: it never confirms, so the full copies stay.
pub(super) async fn propose(node: &Node, journal: &Journal, dir: &Path, me: u64) {
    let metrics = node.raft().metrics().borrow().clone();
    let mut voters: Vec<u64> = metrics.membership_config.membership().voter_ids().collect();
    voters.sort_unstable();
    if voters.len() < 3 {
        return;
    }
    let j = journal.clone();
    let v = voters.clone();
    let plans = tokio::task::spawn_blocking(move || plans(&j, &v)).await;
    match plans {
        Ok(Ok(plans)) => {
            for cmd in plans {
                let _ = node.write(cmd).await;
            }
        }
        Ok(Err(e)) => eprintln!("error: planning seals: {e}"),
        Err(e) => eprintln!("error: planning seals: {e}"),
    }
    for (stream, seal) in journal.reseal_candidates(&voters).unwrap_or_default() {
        match reseal(node, dir, me, stream, &seal, &voters).await {
            Ok(cmd) => {
                let _ = node.write(cmd).await;
            }
            Err(e) => eprintln!("error: planning seals: stream {stream}: {e}"),
        }
    }
}

/// The command re-encoding `stream` over `voters`, from the bytes its current seal's fragments give.
async fn reseal(
    node: &Node,
    dir: &Path,
    me: u64,
    stream: u64,
    seal: &SealRecord,
    voters: &[u64],
) -> Result<Command, BoxError> {
    let got = gather(node, dir, me, stream, seal).await;
    let (s, v) = (seal.clone(), voters.to_vec());
    tokio::task::spawn_blocking(move || {
        let rows = seal::decode(stream, s.len as u64, s.data, s.parity, &got)?;
        seal_command(stream, &rows, &v, true)
    })
    .await?
}

/// `Seal` (or `Reseal`) of `stream`'s `rows` over `voters`.
fn seal_command(
    stream: u64,
    rows: &[u8],
    voters: &[u64],
    again: bool,
) -> Result<Command, BoxError> {
    let (data, parity) = seal::scheme(voters.len());
    let fragments = seal::encode(stream, rows, voters.len())?;
    let (members, len) = (voters.to_vec(), rows.len());
    let checksums = fragments.iter().map(|f| f.checksum).collect();
    Ok(if again {
        Command::Reseal {
            stream,
            members,
            data,
            parity,
            len,
            checksums,
        }
    } else {
        Command::Seal {
            stream,
            members,
            data,
            parity,
            len,
            checksums,
        }
    })
}

fn plans(journal: &Journal, voters: &[u64]) -> Result<Vec<Command>, BoxError> {
    let mut out = vec![];
    let mut streams = journal.ended_unsealed()?;
    let unfinished = journal
        .seals()?
        .into_iter()
        .filter(|(_, s)| s.members != voters);
    streams.extend(
        unfinished
            .filter(|(_, s)| !s.members.iter().all(|m| s.confirmed.contains(m)))
            .map(|(stream, _)| stream),
    );
    for stream in streams {
        let rows = rows_in(&journal.begin_read()?, stream)?;
        out.push(seal_command(stream, &rows, voters, false)?);
    }
    Ok(out)
}

/// Every member: make sure this node's fragment of each seal it belongs to is on disk and valid,
/// rebuilding a missing or corrupt one from the local stream or else from peers' fragments, and
/// confirm the ones not yet confirmed.
pub(super) async fn sweep(node: &Node, journal: &Journal, dir: &Path, me: u64) {
    let (j, d) = (journal.clone(), dir.to_owned());
    let scanned = tokio::task::spawn_blocking(move || store_local(&j, &d, me)).await;
    let (confirm, from_peers) = match scanned {
        Ok(Ok(found)) => found,
        Ok(Err(e)) => return eprintln!("error: node {me}: listing seals: {e}"),
        Err(e) => return eprintln!("error: node {me}: storing fragments: {e}"),
    };
    for build in from_peers {
        // A rebuilt fragment is confirmed by the next tick, which finds it valid on disk.
        if let Err(e) = rebuild(node, dir, me, &build).await {
            eprintln!("error: stream {} node {me}: fragment: {e}", build.0);
        }
    }
    for (stream, pending) in confirm {
        match node
            .propose(Command::ConfirmFragment {
                stream,
                node: me,
                pending,
            })
            .await
        {
            Ok(Ok(Response::Confirmed)) => {}
            other => eprintln!("error: stream {stream} node {me}: confirmation: {other:?}"),
        }
    }
}

/// Confirmations to propose, as (stream, pending), and fragments to rebuild.
type Scan = (Vec<(u64, bool)>, Vec<Build>);

/// A fragment to rebuild: the stream, the seal to read it from, the seal and index to write, and
/// whether the write is for a pending seal.
type Build = (u64, SealRecord, SealRecord, usize, bool);

/// Check this node's fragment of every seal it belongs to, and of every pending one, rewriting it
/// from the local stream when that still matches. Returns the confirmations to propose, and the
/// fragments needing peers.
fn store_local(journal: &Journal, dir: &Path, me: u64) -> Result<Scan, BoxError> {
    let mut found: Scan = (vec![], vec![]);
    for (stream, seal) in journal.seals()? {
        current(journal, dir, me, stream, &seal, &mut found);
        pending(dir, me, stream, &seal, &mut found);
    }
    Ok(found)
}

/// Check this node's fragment of the current `seal`.
fn current(journal: &Journal, dir: &Path, me: u64, stream: u64, seal: &SealRecord, to: &mut Scan) {
    let Some(index) = seal.members.iter().position(|m| *m == me) else {
        return;
    };
    let ok = local(dir, stream, seal, index).is_some()
        || from_rows(journal, dir, stream, seal, index).is_ok();
    match (ok, seal.confirmed.contains(&me)) {
        (true, false) => to.0.push((stream, false)),
        (true, true) => settle(dir, stream, seal, index),
        (false, _) => {
            to.1.push((stream, seal.clone(), seal.clone(), index, false))
        }
    }
}

/// Check this node's fragment of the re-encoding waiting behind `seal`, if it is a member of it.
fn pending(dir: &Path, me: u64, stream: u64, seal: &SealRecord, to: &mut Scan) {
    let Some(next) = seal.pending.as_deref() else {
        return;
    };
    let index = next.members.iter().position(|m| *m == me);
    let Some(index) = index.filter(|_| !next.confirmed.contains(&me)) else {
        return;
    };
    if read_valid(&pending_path(dir, stream, next), stream, next, index).is_some() {
        to.0.push((stream, true));
    } else {
        to.1.push((stream, seal.clone(), next.clone(), index, true));
    }
}

fn from_rows(
    journal: &Journal,
    dir: &Path,
    stream: u64,
    seal: &SealRecord,
    index: usize,
) -> Result<(), BoxError> {
    let rows = rows_in(&journal.begin_read()?, stream)?;
    write_checked(&dir.join(stream.to_string()), stream, seal, index, &rows)
}

/// Encode `rows` and write fragment `index` to `path` if it matches the replicated checksum.
fn write_checked(
    path: &Path,
    stream: u64,
    seal: &SealRecord,
    index: usize,
    rows: &[u8],
) -> Result<(), BoxError> {
    let fragment = seal::encode(stream, rows, seal.members.len())?.swap_remove(index);
    if fragment.checksum != seal.checksums[index] {
        return Err("stream does not match the replicated checksum".into());
    }
    write(path, &fragment.bytes)
}

/// Rebuild this node's fragment `index` of `target` from the valid fragments of `source`: the
/// same seal, or the current one when `target` is pending.
async fn rebuild(node: &Node, dir: &Path, me: u64, build: &Build) -> Result<(), BoxError> {
    let (stream, source, target, index, pending) = (build.0, &build.1, &build.2, build.3, build.4);
    let got = gather(node, dir, me, stream, source).await;
    let path = if pending {
        pending_path(dir, stream, target)
    } else {
        dir.join(stream.to_string())
    };
    let (s, t) = (source.clone(), target.clone());
    tokio::task::spawn_blocking(move || {
        let rows = seal::decode(stream, s.len as u64, s.data, s.parity, &got)?;
        write_checked(&path, stream, &t, index, &rows)
    })
    .await?
}

/// Where a pending re-encoding's fragment waits until its seal is promoted.
fn pending_path(dir: &Path, stream: u64, seal: &SealRecord) -> PathBuf {
    dir.join(format!("{stream}.{}-{}", seal.data, seal.parity))
}

fn read_valid(path: &Path, stream: u64, seal: &SealRecord, index: usize) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    (seal::checksum(stream, index, seal.len as u64, &bytes) == seal.checksums[index])
        .then_some(bytes)
}

/// The bytes of this node's file for fragment `index` of `stream`, when they match the checksum.
/// A promoted seal's fragment may still be waiting under its pending name.
pub fn local(dir: &Path, stream: u64, seal: &SealRecord, index: usize) -> Option<Vec<u8>> {
    read_valid(&dir.join(stream.to_string()), stream, seal, index)
        .or_else(|| read_valid(&pending_path(dir, stream, seal), stream, seal, index))
}

/// Move a promoted seal's fragment from its pending name into place.
fn settle(dir: &Path, stream: u64, seal: &SealRecord, index: usize) {
    let (waiting, path) = (
        pending_path(dir, stream, seal),
        dir.join(stream.to_string()),
    );
    if read_valid(&waiting, stream, seal, index).is_some()
        && std::fs::rename(&waiting, &path).is_ok()
    {
        let _ = std::fs::File::open(dir).and_then(|d| d.sync_all());
    }
}

/// Fragments of the sealed `stream` that match the replicated checksums, enough to rebuild it
/// when they can be had: this node's own first, then peers' fetched concurrently, all within
/// `REQUEST_DEADLINE`. A fragment that is missing, late or does not match is skipped.
pub(crate) async fn gather(
    node: &Node,
    dir: &Path,
    me: u64,
    stream: u64,
    seal: &SealRecord,
) -> Vec<seal::Fragment> {
    let fragment = |index: usize, bytes: Vec<u8>| seal::Fragment {
        index,
        checksum: seal.checksums[index],
        bytes,
    };
    let mut got: Vec<seal::Fragment> = seal
        .members
        .iter()
        .position(|m| *m == me)
        .and_then(|i| local(dir, stream, seal, i).map(|b| fragment(i, b)))
        .into_iter()
        .collect();
    let peers = node.raft().metrics().borrow().membership_config.clone();
    let mut fetches = tokio::task::JoinSet::new();
    for (index, member) in seal.members.iter().enumerate().filter(|(_, m)| **m != me) {
        let Some(peer) = peers.membership().get_node(member) else {
            continue;
        };
        let (client, addr) = (node.http.clone(), peer.addr.clone());
        fetches.spawn(async move {
            (
                index,
                super::network::fetch_fragment(&client, &addr, stream).await,
            )
        });
    }
    let collect = async {
        while got.len() < seal.data {
            let Some(Ok((index, Some(bytes)))) = fetches.join_next().await else {
                if fetches.is_empty() {
                    break;
                }
                continue;
            };
            if seal::checksum(stream, index, seal.len as u64, &bytes) == seal.checksums[index] {
                got.push(fragment(index, bytes));
            }
        }
    };
    let _ = tokio::time::timeout(super::REQUEST_DEADLINE, collect).await;
    got
}

/// Durably replace the file at `path` with `bytes`.
fn write(path: &Path, bytes: &[u8]) -> Result<(), BoxError> {
    let dir = path.parent().ok_or("fragment path has no directory")?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let file = std::fs::File::create(&tmp)?;
    std::io::Write::write_all(&mut &file, bytes)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
}
