//! `rs3 acp <node-url>...`: an Agent Client Protocol v1 agent over newline-delimited
//! JSON-RPC 2.0 on stdio. A prompt becomes a goal on a node and its output streams back
//! as message chunks. Stdout carries protocol only; diagnostics go to stderr.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::JoinSet;

/// Seconds a call keeps trying while the cluster is unavailable, unless overridden.
const DEFAULT_UNAVAILABLE_TIMEOUT: u64 = 300;
const UNAVAILABLE_TIMEOUT_ENV: &str = "ACP_UNAVAILABLE_TIMEOUT";
const RETRY_PAUSE: Duration = Duration::from_millis(100);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const NODE_TIMEOUT: Duration = Duration::from_secs(6);
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const CLUSTER_UNAVAILABLE: i64 = -32001;
const TURN_IN_FLIGHT: i64 = -32002;
const GOAL_FAILED: i64 = -32000;
const UNKNOWN_SCRIPT: i64 = -32003;

/// Whole lines for the one thread that owns stdout.
type Out = Sender<String>;

fn send(out: &Out, message: Value) {
    let _ = out.send(message.to_string());
}

fn respond(out: &Out, id: &Value, result: Value) {
    send(out, json!({"jsonrpc": "2.0", "id": id, "result": result}));
}

fn fail(out: &Out, id: &Value, code: i64, message: &str) {
    let error = json!({"code": code, "message": message});
    send(out, json!({"jsonrpc": "2.0", "id": id, "error": error}));
}

/// Answer request `id` with a named error: the name is both the message and `data.error`.
fn fail_named(out: &Out, id: &Value, code: i64, name: &str) {
    let error = json!({"code": code, "message": name, "data": {"error": name}});
    send(out, json!({"jsonrpc": "2.0", "id": id, "error": error}));
}

/// A `session/update` notification carrying `update`.
fn notify(out: &Out, session: &str, update: Value) {
    let params = json!({"sessionId": session, "update": update});
    send(
        out,
        json!({"jsonrpc": "2.0", "method": "session/update", "params": params}),
    );
}

/// A `session/update` carrying `text`; `kind` is `user_message_chunk`, `agent_message_chunk` or `agent_thought_chunk`.
fn chunk(out: &Out, session: &str, kind: &str, text: &str) {
    let content = json!({"type": "text", "text": text});
    notify(
        out,
        session,
        json!({"sessionUpdate": kind, "content": content}),
    );
}

/// Tell the client which commands session `session` understands.
fn advertise(out: &Out, session: &str) {
    let run =
        json!({"name": "run", "description": "Run a stored script", "input": {"hint": "<key>"}});
    let code =
        json!({"name": "code", "description": "Write and test code", "input": {"hint": "<task>"}});
    let update =
        json!({"sessionUpdate": "available_commands_update", "availableCommands": [run, code]});
    notify(out, session, update);
}

fn spawn_writer(lines: std::sync::mpsc::Receiver<String>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut stdout = std::io::stdout();
        for line in lines {
            if writeln!(stdout, "{line}")
                .and_then(|()| stdout.flush())
                .is_err()
            {
                break;
            }
        }
    })
}

fn spawn_reader() -> tokio::sync::mpsc::UnboundedReceiver<Vec<u8>> {
    let (sent, lines) = unbounded_channel();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        loop {
            let mut line = Vec::new();
            match stdin.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if sent.send(line).is_err() {
                        break;
                    }
                }
            }
        }
    });
    lines
}

/// Why a call ended: the cluster could not answer in time, a node gave a final answer, or a
/// node refused with a status.
enum CallError {
    Unavailable(String),
    Final(String),
    /// A node answered this non-success status, which is final.
    Refused(StatusCode, String),
    /// The goal was cancelled, by the client's `session/cancel` or by someone else.
    Cancelled,
    /// An error with its own JSON-RPC code and name.
    Named(i64, String),
}

/// The nodes of the cluster and the one way to call them.
#[derive(Clone)]
struct Nodes {
    client: Client,
    urls: Vec<String>,
    /// How long one call keeps trying before it answers unavailable.
    wait: Duration,
    /// The node that answered last; each call starts there.
    preferred: Arc<AtomicUsize>,
}

/// A 503 with one of these errors is the cluster being briefly unable; any other is final.
const RETRIED_503: [&str; 4] = [
    "no_quorum",
    "unavailable",
    "outcome_unknown",
    "fragments_unavailable",
];

impl Nodes {
    /// One request to the first node that answers, starting at the last one that did. Connection
    /// errors, timeouts, unreadable 2xx bodies and a 503 from `RETRIED_503` move on to the next
    /// node until `wait` has passed since the call began. Any other status is final.
    /// Errors read `METHOD path: cause`.
    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, CallError> {
        let deadline = Instant::now() + self.wait;
        let mut index = self.preferred.load(Ordering::Relaxed);
        loop {
            let node = index % self.urls.len();
            let url = &self.urls[node];
            let left = deadline.saturating_duration_since(Instant::now());
            let url = format!("{url}{path}");
            let limit = left.min(NODE_TIMEOUT);
            match self.attempt(&method, &url, body, limit).await {
                Ok(value) => {
                    self.preferred.store(node, Ordering::Relaxed);
                    return Ok(value);
                }
                Err(CallError::Unavailable(cause)) => {
                    let cause = format!("{method} {path}: {cause}");
                    if Instant::now() >= deadline {
                        return Err(CallError::Unavailable(cause));
                    }
                    eprintln!("{cause}");
                    tokio::time::sleep(RETRY_PAUSE).await;
                    index += 1;
                }
                Err(CallError::Final(cause)) => {
                    return Err(CallError::Final(format!("{method} {path}: {cause}")));
                }
                Err(CallError::Refused(status, cause)) => {
                    let cause = format!("{method} {path}: {cause}");
                    return Err(CallError::Refused(status, cause));
                }
                Err(CallError::Cancelled) => return Err(CallError::Cancelled),
                Err(named @ CallError::Named(..)) => return Err(named),
            }
        }
    }

    async fn attempt(
        &self,
        method: &Method,
        url: &str,
        body: Option<&Value>,
        limit: Duration,
    ) -> Result<Value, CallError> {
        let mut request = self.client.request(method.clone(), url).timeout(limit);
        if let Some(body) = body {
            request = request.json(body);
        }
        let retry = |e: reqwest::Error| CallError::Unavailable(e.to_string());
        let reply = request.send().await.map_err(retry)?;
        let status = reply.status();
        if status.is_success() {
            return reply.json().await.map_err(retry);
        }
        if status == StatusCode::SERVICE_UNAVAILABLE {
            let error = reply.json::<Value>().await.ok();
            let error = error.as_ref().and_then(|b| b["error"].as_str());
            if error.is_some_and(|e| RETRIED_503.contains(&e)) {
                return Err(CallError::Unavailable(status.to_string()));
            }
        }
        Err(CallError::Refused(status, status.to_string()))
    }
}

/// Answer request `id` with the error a failed call ends in, and log its cause.
fn fail_call(out: &Out, id: &Value, error: CallError) {
    match error {
        CallError::Unavailable(cause) => {
            eprintln!("{cause}");
            fail_named(out, id, CLUSTER_UNAVAILABLE, "cluster_unavailable");
        }
        CallError::Final(cause) | CallError::Refused(_, cause) => {
            eprintln!("{cause}");
            fail(out, id, INTERNAL_ERROR, &cause);
        }
        CallError::Cancelled => respond(out, id, json!({"stopReason": "cancelled"})),
        CallError::Named(code, name) => fail_named(out, id, code, &name),
    }
}

const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

/// Whether byte `offset` of `output` lies inside a think block.
fn in_think(output: &str, offset: usize) -> bool {
    let seen = &output[..offset];
    seen.rfind(THINK_OPEN) > seen.rfind(THINK_CLOSE)
}

/// How many bytes at the end of `text` are a proper prefix of `tag`.
fn partial_tag(text: &str, tag: &str) -> usize {
    (1..tag.len())
        .rev()
        .find(|&k| text.ends_with(&tag[..k]))
        .unwrap_or(0)
}

/// Split `text` into (update kind, piece) with the think tags dropped and empty (or, inside think, blank) pieces skipped,
/// starting inside a think block when `thinking`. Unless `last`, a trailing part of a tag is
/// not consumed. Also returns the number of bytes consumed.
fn split_think(text: &str, mut thinking: bool, last: bool) -> (Vec<(&'static str, &str)>, usize) {
    let mut chunks = Vec::new();
    let mut rest = text;
    loop {
        let (tag, kind) = if thinking {
            (THINK_CLOSE, "agent_thought_chunk")
        } else {
            (THINK_OPEN, "agent_message_chunk")
        };
        let found = rest.find(tag);
        let held = if found.is_some() || last {
            0
        } else {
            partial_tag(rest, tag)
        };
        let end = found.unwrap_or(rest.len() - held);
        let piece = &rest[..end];
        if !(piece.is_empty() || thinking && piece.trim().is_empty()) {
            chunks.push((kind, piece));
        }
        let Some(at) = found else {
            rest = &rest[end..];
            break;
        };
        rest = &rest[at + tag.len()..];
        thinking = !thinking;
    }
    (chunks, text.len() - rest.len())
}

/// How much of a goal the client has received: whole items (replies, checks, runs) and the byte
/// offset into the current reply, or into the output of a text goal.
#[derive(Clone, Copy, Default)]
struct Progress {
    item: usize,
    offset: usize,
}

/// A `tool_call` and its closing `tool_call_update` for one committed step of a coding goal.
fn tool_call(id: &str, title: &str, kind: &str, failed: bool, lines: &[String]) -> [Value; 2] {
    let status = if failed { "failed" } else { "completed" };
    let text = json!({"type": "content", "content": {"type": "text", "text": lines.join("\n")}});
    [
        json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": title, "kind": kind, "status": "in_progress"}),
        json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": status, "content": [text]}),
    ]
}

/// The type check of iteration `n` of goal `goal`.
fn check_calls(goal: u64, n: usize, iteration: &Value) -> [Value; 2] {
    let found = iteration["diagnostics"].as_array().into_iter().flatten();
    let mut lines: Vec<String> = found
        .map(|d| {
            let (file, code) = (d["file"].as_str().unwrap_or_default(), &d["code"]);
            let at = format!("{}:{}", d["line"], d["column"]);
            let message = d["message"].as_str().unwrap_or_default();
            format!("{file}:{at} TS{code}: {message}")
        })
        .collect();
    let failed = !lines.is_empty();
    if !failed {
        lines.push("no type errors".into());
    }
    tool_call(
        &format!("goal-{goal}-{n}-check"),
        "Type check",
        "other",
        failed,
        &lines,
    )
}

/// The test run of iteration `n` of goal `goal`.
fn run_calls(goal: u64, n: usize, run: &Value, iteration: &Value) -> [Value; 2] {
    let mut lines = Vec::new();
    for t in iteration["tests"].as_array().into_iter().flatten() {
        let name = t["name"].as_str().unwrap_or_default();
        lines.push(match (t["passed"].as_bool(), t["error"].as_str()) {
            (Some(true), _) => format!("PASS {name}"),
            (_, Some(error)) => format!("FAIL {name}: {error}"),
            _ => format!("FAIL {name}"),
        });
    }
    let logs = iteration["logs"].as_array().into_iter().flatten();
    lines.extend(logs.filter_map(|l| l.as_str().map(str::to_owned)));
    let status = run["status"].as_str().unwrap_or_default();
    if matches!(status, "error" | "timeout") {
        lines.push(match run["error"].as_str() {
            Some(error) => format!("sandbox {status}: {error}"),
            None => format!("sandbox {status}"),
        });
    }
    let title = "Run tests";
    tool_call(
        &format!("goal-{goal}-{n}-run"),
        title,
        "execute",
        status != "passed",
        &lines,
    )
}

/// What the client has not yet seen of coding goal `goal`, as `session/update` payloads, and the
/// progress after it: each attempt's reply as message chunks, then the type check and the test
/// run as tool calls once committed. `ended` is true once the goal is done. A reply still being
/// written holds back a trailing U+FFFD, which may be half a character.
fn code_updates(goal: &Value, id: u64, from: Progress, ended: bool) -> (Vec<Value>, Progress) {
    let iterations = goal["iterations"].as_array().map_or(&[][..], Vec::as_slice);
    // Only the last reply of a running goal's unchecked last iteration can still grow.
    let running = !ended && iterations.last().is_some_and(|i| i["checked"] != true);
    let mut items = Vec::new();
    for (at, iteration) in iterations.iter().enumerate() {
        let replies = iteration["replies"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        for (k, reply) in replies.iter().enumerate() {
            let text = reply.as_str().unwrap_or_default();
            let growing = running && at + 1 == iterations.len() && k + 1 == replies.len();
            items.push((vec![], Some((text, !growing))));
        }
        let mut calls = Vec::new();
        if iteration["checked"] == true {
            calls.extend(check_calls(id, at + 1, iteration));
        }
        if iteration["run"].is_object() {
            calls.extend(run_calls(id, at + 1, &iteration["run"], iteration));
        }
        items.extend(calls.chunks(2).map(|pair| (pair.to_vec(), None)));
    }
    let (mut updates, mut now) = (Vec::new(), from);
    for (calls, reply) in items.into_iter().skip(from.item) {
        if let Some((text, complete)) = reply {
            let tail = text.get(now.offset..).unwrap_or_default();
            let tail = if complete {
                tail
            } else {
                tail.strip_suffix('\u{FFFD}').unwrap_or(tail)
            };
            if !tail.is_empty() {
                let content = json!({"type": "text", "text": tail});
                updates.push(json!({"sessionUpdate": "agent_message_chunk", "content": content}));
                now.offset += tail.len();
            }
            if !complete {
                break;
            }
        }
        updates.extend(calls);
        now = Progress {
            item: now.item + 1,
            offset: 0,
        };
    }
    (updates, now)
}

/// Follow goal `id` from `progress`, handing the goal, the progress already delivered and whether
/// the goal is done to `emit`. `emit` returns the progress the client has now received up to. While `cancelled` is true each round
/// is a DELETE, which is idempotent and answers like a GET; a goal that ends then, or is
/// cancelled by anyone, ends the stream as `Cancelled`.
async fn stream_goal(
    nodes: &Nodes,
    id: u64,
    mut progress: Progress,
    emit: impl Fn(&Value, Progress, bool) -> Progress,
    cancelled: impl Fn() -> bool,
) -> Result<Option<String>, CallError> {
    let path = format!("/goals/{id}");
    loop {
        let method = if cancelled() {
            Method::DELETE
        } else {
            Method::GET
        };
        let goal = nodes.call(method, &path, None).await?;
        let status = goal["status"].as_str();
        progress = emit(&goal, progress, status == Some("done"));
        match status {
            Some("done" | "failed" | "cancelled") if cancelled() => {
                return Err(CallError::Cancelled);
            }
            Some("done") => {
                // A coding goal that did not succeed stops like a goal that ran out of budget.
                let unmet = goal["outcome"].as_str().filter(|o| *o != "success");
                let reason = goal["reason"].as_str().or(unmet.map(|_| "budget"));
                return Ok(reason.map(str::to_owned));
            }
            Some("cancelled") => return Err(CallError::Cancelled),
            Some("failed") => {
                let error = goal["error"].as_str().unwrap_or_default();
                return Err(CallError::Named(GOAL_FAILED, error.to_owned()));
            }
            _ => tokio::time::sleep(POLL_INTERVAL).await,
        }
    }
}

/// What the adapter knows of one session.
#[derive(Clone, Copy, Default)]
struct Session {
    next_turn: u64,
    /// A goal whose output the client has only received up to the given byte.
    unfinished: Option<(u64, Progress)>,
    /// A prompt is being answered; a second one is refused.
    in_flight: bool,
    /// `session/cancel` arrived while the prompt was in flight.
    cancel: bool,
}

/// Session id to its state, shared by every task.
#[derive(Clone, Default)]
struct Sessions(Arc<Mutex<HashMap<String, Session>>>);

impl Sessions {
    fn get(&self, session: &str) -> Option<Session> {
        let map = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        map.get(session).copied()
    }

    fn set(&self, session: &str, state: Session) {
        let mut map = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        map.insert(session.to_owned(), state);
    }

    fn update(&self, session: &str, change: impl FnOnce(&mut Session)) {
        let mut map = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(state) = map.get_mut(session) {
            change(state);
        }
    }
}

struct Turn {
    out: Out,
    nodes: Nodes,
    sessions: Sessions,
    id: Value,
    session: String,
    prompt: String,
}

impl Turn {
    fn cancel_asked(&self) -> bool {
        self.sessions.get(&self.session).is_some_and(|s| s.cancel)
    }

    /// Follow `goal` from `offset`, keeping the session's record of how far the client has
    /// read. Only a final answer clears it; an unavailable cluster keeps it for the next prompt.
    async fn follow(&self, goal: u64, from: Progress) -> Result<Option<String>, CallError> {
        let emit = |status: &Value, from: Progress, last: bool| {
            let read = if status["iterations"].is_array() {
                let (updates, read) = code_updates(status, goal, from, last);
                for update in updates {
                    notify(&self.out, &self.session, update);
                }
                read
            } else {
                let output = status["output"].as_str().unwrap_or_default();
                let Some(text) = output.get(from.offset..) else {
                    return from;
                };
                let (chunks, used) = split_think(text, in_think(output, from.offset), last);
                for (kind, piece) in chunks {
                    chunk(&self.out, &self.session, kind, piece);
                }
                Progress {
                    offset: from.offset + used,
                    ..from
                }
            };
            self.sessions
                .update(&self.session, |s| s.unfinished = Some((goal, read)));
            read
        };
        let cancelled = || self.cancel_asked();
        let result = stream_goal(&self.nodes, goal, from, emit, cancelled).await;
        if !matches!(result, Err(CallError::Unavailable(_))) {
            self.sessions.update(&self.session, |s| s.unfinished = None);
        }
        result
    }

    /// Finish the goal a timed-out prompt left behind, then start the next turn of the
    /// session and follow its goal. The turn number moves on only once the node has answered,
    /// so a failed call is resent as the same turn.
    async fn stream(&self) -> Result<Option<String>, CallError> {
        let state = self.sessions.get(&self.session).unwrap_or_default();
        if let Some((goal, progress)) = state.unfinished {
            self.finish_earlier(goal, progress).await?;
        }
        let mut number = state.next_turn;
        let path = format!("/sessions/{}/turns", self.session);
        let started = loop {
            let body = json!({"turn": number, "prompt": self.prompt});
            match self.nodes.call(Method::POST, &path, Some(&body)).await {
                Err(CallError::Refused(StatusCode::CONFLICT, _)) => {
                    self.catch_up(number).await?;
                    number += 1;
                }
                Err(CallError::Refused(StatusCode::NOT_FOUND, _)) => {
                    return Err(CallError::Named(UNKNOWN_SCRIPT, "unknown_script".into()));
                }
                started => break started?,
            }
        };
        let goal = started["goal"]
            .as_u64()
            .ok_or_else(|| CallError::Final(format!("POST {path}: no goal id")))?;
        let unfinished = Some((goal, Progress::default()));
        self.sessions.update(&self.session, |s| {
            s.next_turn = number + 1;
            s.unfinished = unfinished;
        });
        self.follow(goal, Progress::default()).await
    }

    /// Turn `number` is already committed by someone else: stream its goal and move past it.
    async fn catch_up(&self, number: u64) -> Result<(), CallError> {
        let path = format!("/sessions/{}", self.session);
        let listed = self.nodes.call(Method::GET, &path, None).await?;
        let goal = listed["turns"][number as usize]["goal"]
            .as_u64()
            .ok_or_else(|| CallError::Final(format!("GET {path}: no turn {number}")))?;
        self.sessions.update(&self.session, |s| {
            s.next_turn = number + 1;
            s.unfinished = Some((goal, Progress::default()));
        });
        self.finish_earlier(goal, Progress::default()).await
    }

    /// Deliver the rest of an earlier turn. How that turn ended is its own result, so only an
    /// outage or a cancel of this prompt keeps this prompt from running.
    async fn finish_earlier(&self, goal: u64, from: Progress) -> Result<(), CallError> {
        match self.follow(goal, from).await {
            Err(CallError::Final(_) | CallError::Named(..) | CallError::Cancelled)
                if !self.cancel_asked() =>
            {
                Ok(())
            }
            result => result.map(drop),
        }
    }

    async fn run(self) {
        let result = self.stream().await;
        self.sessions.update(&self.session, |s| {
            s.in_flight = false;
            s.cancel = false;
        });
        match result {
            Ok(reason) => {
                let stop = if reason.as_deref() == Some("budget") {
                    "max_tokens"
                } else {
                    "end_turn"
                };
                respond(&self.out, &self.id, json!({"stopReason": stop}));
            }
            Err(error) => fail_call(&self.out, &self.id, error),
        }
    }
}

/// The prompt as one string: text blocks as written, resource links as their URI, the
/// baseline block types of ACP. Other block types are ignored.
fn prompt_text(params: &Value) -> String {
    let blocks = params["prompt"].as_array().into_iter().flatten();
    blocks
        .filter_map(|b| match b["type"].as_str() {
            Some("text") => b["text"].as_str(),
            Some("resource_link") => b["uri"].as_str(),
            _ => None,
        })
        .collect()
}

/// The (prompt, output, goal while still running) of every turn of session `number`, read goal
/// by goal; None when the node does not know the session.
type Loaded = Vec<(String, String, Option<u64>)>;

async fn load_turns(nodes: &Nodes, number: u64) -> Result<Option<Loaded>, CallError> {
    let path = format!("/sessions/{number}");
    let listed = match nodes.call(Method::GET, &path, None).await {
        Err(CallError::Refused(StatusCode::NOT_FOUND, _)) => return Ok(None),
        listed => listed?,
    };
    let bad = |what: &str| CallError::Final(format!("GET {path}: {what}"));
    let mut turns = Vec::new();
    for turn in listed["turns"].as_array().ok_or_else(|| bad("no turns"))? {
        let id = turn["goal"]
            .as_u64()
            .ok_or_else(|| bad("turn without a goal id"))?;
        let goal = nodes
            .call(Method::GET, &format!("/goals/{id}"), None)
            .await?;
        let prompt = turn["prompt"].as_str().unwrap_or_default();
        let output = goal["output"].as_str().unwrap_or_default();
        let ended = matches!(
            goal["status"].as_str(),
            Some("done" | "failed" | "cancelled")
        );
        turns.push((prompt.to_owned(), output.to_owned(), (!ended).then_some(id)));
    }
    Ok(Some(turns))
}

/// Replay `turns` to the client, adopt the session and answer the load. The session is set
/// before the response so that a prompt sent on receipt finds it.
fn adopt(out: &Out, sessions: &Sessions, id: &Value, number: u64, turns: &Loaded) {
    let session = number.to_string();
    let mut unfinished = None;
    for (prompt, output, running) in turns {
        chunk(out, &session, "user_message_chunk", prompt);
        let (chunks, used) = split_think(output, false, running.is_none());
        for (kind, piece) in chunks {
            chunk(out, &session, kind, piece);
        }
        let at = Progress {
            offset: used,
            ..Progress::default()
        };
        unfinished = running.map(|goal| (goal, at)).or(unfinished);
    }
    let next_turn = turns.len() as u64;
    sessions.set(
        &session,
        Session {
            next_turn,
            unfinished,
            ..Session::default()
        },
    );
    respond(out, id, Value::Null);
    advertise(out, &session);
}

struct Adapter {
    out: Out,
    nodes: Nodes,
    sessions: Sessions,
    turns: JoinSet<()>,
}

impl Adapter {
    fn handle(&mut self, line: &[u8]) {
        let Ok(message) = serde_json::from_slice::<Value>(line) else {
            return fail(&self.out, &Value::Null, PARSE_ERROR, "parse error");
        };
        let id = &message["id"];
        let Some(method) = message["method"].as_str() else {
            return fail(&self.out, id, INVALID_REQUEST, "not a JSON-RPC request");
        };
        if id.is_null() {
            if method == "session/cancel" {
                let session = message["params"]["sessionId"].as_str().unwrap_or_default();
                self.sessions.update(session, |s| s.cancel = s.in_flight);
            }
            return;
        }
        match method {
            "initialize" => respond(&self.out, id, initialized()),
            "session/new" => self.new_session(id),
            "session/load" => self.load_session(id, &message["params"]),
            "session/prompt" => self.prompt(id, &message["params"]),
            _ => fail(&self.out, id, METHOD_NOT_FOUND, "method not found"),
        }
    }

    fn new_session(&mut self, id: &Value) {
        let (out, nodes, sessions, id) = (
            self.out.clone(),
            self.nodes.clone(),
            self.sessions.clone(),
            id.clone(),
        );
        self.turns.spawn(async move {
            let created = nodes.call(Method::POST, "/sessions", None).await;
            let created = created.and_then(|c| {
                let no_id = || CallError::Final("POST /sessions: no id".into());
                c["id"].as_u64().ok_or_else(no_id)
            });
            match created {
                Ok(session) => {
                    let session = session.to_string();
                    sessions.set(&session, Session::default());
                    respond(&out, &id, json!({"sessionId": session}));
                    advertise(&out, &session);
                }
                Err(error) => fail_call(&out, &id, error),
            }
        });
    }

    /// Rebuild a session from the cluster: replay every turn as a user chunk with the prompt and
    /// an agent chunk with the whole output, then adopt the session. Nothing is emitted unless
    /// every read succeeded, and the session is set before the response so that a prompt sent
    /// on receipt finds it.
    fn load_session(&mut self, id: &Value, params: &Value) {
        let number = params["sessionId"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok());
        let Some(number) = number else {
            return fail(&self.out, id, INVALID_PARAMS, "unknown session");
        };
        if self
            .sessions
            .get(&number.to_string())
            .is_some_and(|s| s.in_flight)
        {
            return fail_named(&self.out, id, TURN_IN_FLIGHT, "turn_in_flight");
        }
        let (out, nodes, sessions, id) = (
            self.out.clone(),
            self.nodes.clone(),
            self.sessions.clone(),
            id.clone(),
        );
        self.turns.spawn(async move {
            match load_turns(&nodes, number).await {
                Ok(Some(loaded)) => adopt(&out, &sessions, &id, number, &loaded),
                Ok(None) => fail(&out, &id, INVALID_PARAMS, "unknown session"),
                Err(error) => fail_call(&out, &id, error),
            }
        });
    }

    fn prompt(&mut self, id: &Value, params: &Value) {
        let session = params["sessionId"].as_str().unwrap_or_default();
        let Some(state) = self.sessions.get(session) else {
            return fail(&self.out, id, INVALID_PARAMS, "unknown session");
        };
        if !params["prompt"].is_array() {
            return fail(&self.out, id, INVALID_PARAMS, "prompt must be an array");
        }
        if state.in_flight {
            return fail_named(&self.out, id, TURN_IN_FLIGHT, "turn_in_flight");
        }
        self.sessions.update(session, |s| s.in_flight = true);
        let turn = Turn {
            out: self.out.clone(),
            nodes: self.nodes.clone(),
            sessions: self.sessions.clone(),
            id: id.clone(),
            session: session.to_owned(),
            prompt: prompt_text(params),
        };
        self.turns.spawn(turn.run());
    }
}

fn initialized() -> Value {
    json!({
        "protocolVersion": 1,
        "agentCapabilities": {"loadSession": true},
        "authMethods": [],
        "agentInfo": {"name": "rs3", "version": env!("CARGO_PKG_VERSION")},
    })
}

/// Serve ACP on stdio until stdin closes. Goals already started keep running on the node.
pub async fn run(urls: &[String]) -> Result<(), String> {
    if urls.is_empty() {
        return Err("usage: rs3 acp <node-url>...".into());
    }
    let wait = match std::env::var(UNAVAILABLE_TIMEOUT_ENV) {
        Ok(value) => value.parse().map_err(|_| {
            format!("{UNAVAILABLE_TIMEOUT_ENV} must be whole seconds, got {value:?}")
        })?,
        Err(_) => DEFAULT_UNAVAILABLE_TIMEOUT,
    };
    let client = Client::builder()
        .timeout(NODE_TIMEOUT)
        .build()
        .map_err(|e| format!("cannot build HTTP client: {e}"))?;
    let (out, lines) = channel();
    let writer = spawn_writer(lines);
    let mut input = spawn_reader();
    let mut adapter = Adapter {
        out,
        nodes: Nodes {
            client,
            urls: urls
                .iter()
                .map(|u| u.trim_end_matches('/').to_owned())
                .collect(),
            wait: Duration::from_secs(wait),
            preferred: Arc::default(),
        },
        sessions: Sessions::default(),
        turns: JoinSet::new(),
    };
    while let Some(line) = input.recv().await {
        adapter.handle(&line);
    }
    adapter.turns.shutdown().await;
    drop(adapter);
    let _ = writer.join();
    Ok(())
}
