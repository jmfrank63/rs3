//! The coding loop's text, ported byte for byte from iso-agents `loop.ts` and `extractor.ts`:
//! prompts, reply extraction and the feedback the model gets back.

use crate::checker::Diagnostic;
use crate::journal::Record;
use crate::model::replay_batch;
use crate::sandbox::RunOutput;
use serde_json::Value;

pub const SYSTEM_PROMPT: &str = r#"You are a senior TypeScript engineer implementing a small pure function.
Reply with ONLY a JSON object, no prose around it, exactly:
{"explanation": "<what you did>", "files": {"<filename>": "<full file content>"}}
Hard rules:
- The code must type-check cleanly under strict ES2022.
- Every test must pass.
- Return exactly ONE script-style file: NO import or export statements anywhere.
- No async, no await, no Promise (the runtime is synchronous).
- The runtime injects test(name, fn), expect(actual).toEqual/toBeTrue/toBeCloseTo, assert(cond, msg?) and console.
- Include test("name", () => { expect(...) }) calls that cover the spec.
- Return the FULL content of every file every time, never a diff."#;

pub const CORRECTIVE_PROMPT: &str = "respond with ONLY valid JSON";
pub const FEEDBACK_WINDOW: usize = 3;
pub const MAX_GUEST_SOURCE_BYTES: usize = 20480;
const ONE_FILE: &str = "return exactly ONE file (the sandbox executes a single script)";
const FIX_AGAIN: &str = "fix and return the FULL file(s) again";

/// How a goal ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    MaxIterations,
    LlmError,
    LlmMalformed,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Success => "success",
            Outcome::MaxIterations => "max_iterations",
            Outcome::LlmError => "llm_error",
            Outcome::LlmMalformed => "llm_malformed",
        }
    }
}

/// A reply that cannot become files. `Malformed` earns one corrective retry and then ends the
/// goal; `Violation` breaks a sandbox rule and heals through feedback, never terminally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    Malformed(String),
    Violation(String),
}

impl Reject {
    /// The feedback the model gets for a violation.
    pub fn feedback(&self) -> String {
        match self {
            Reject::Malformed(m) | Reject::Violation(m) => format!("{m}; {FIX_AGAIN}"),
        }
    }
}

pub fn user_message(task: &str, spec: &str) -> String {
    format!("{task}\n\nSPEC:\n{spec}\n\nReturn ONLY the JSON contract.")
}

/// Only the last three feedback messages reach the model.
pub fn feedback_window(feedback: &[String]) -> &[String] {
    &feedback[feedback.len().saturating_sub(FEEDBACK_WINDOW)..]
}

/// The chat-template text for one model call: system, task, windowed feedback, and on a
/// corrective retry the corrective turn.
pub fn prompt(task: &str, spec: &str, feedback: &[String], corrective: bool) -> String {
    let mut turns = vec![
        ("system", SYSTEM_PROMPT.to_string()),
        ("user", user_message(task, spec)),
    ];
    turns.extend(
        feedback_window(feedback)
            .iter()
            .map(|f| ("user", f.clone())),
    );
    if corrective {
        turns.push(("user", CORRECTIVE_PROMPT.to_string()));
    }
    let mut text: String = turns
        .iter()
        .map(|(role, body)| format!("<|im_start|>{role}\n{body}<|im_end|>\n"))
        .collect();
    text.push_str("<|im_start|>assistant\n");
    text
}

/// A leading fence is stripped; a missing trailing fence still parses through brace salvage.
fn strip_fences(text: &str) -> &str {
    let t = text.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return t;
    };
    let Some((_, body)) = rest.split_once('\n') else {
        return t;
    };
    let body = body.trim();
    body.strip_suffix("```").unwrap_or(body).trim()
}

fn parse(text: &str) -> Result<Value, Reject> {
    let s = strip_fences(text);
    if let Ok(v) = serde_json::from_str(s) {
        return Ok(v);
    }
    let (Some(first), Some(last)) = (s.find('{'), s.rfind('}')) else {
        return Err(Reject::Malformed("response contains no JSON object".into()));
    };
    if last <= first {
        return Err(Reject::Malformed("response contains no JSON object".into()));
    }
    serde_json::from_str(&s[first..=last]).map_err(|_| {
        Reject::Malformed("response is not valid JSON (fenced, bare, or brace-salvaged)".into())
    })
}

fn has_word(source: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    source.match_indices(word).any(|(i, _)| {
        !source[..i].chars().next_back().is_some_and(is_word)
            && !source[i + word.len()..].chars().next().is_some_and(is_word)
    })
}

/// The reply's files as `(name, content)` pairs: exactly one script-style file within the size limit.
pub fn extract(reply: &str) -> Result<Vec<(String, String)>, Reject> {
    let malformed = |m: &str| Reject::Malformed(m.into());
    let value = parse(reply)?;
    let Some(object) = value.as_object() else {
        return Err(malformed("contract is not a JSON object"));
    };
    let Some(files) = object.get("files").and_then(Value::as_object) else {
        return Err(malformed(
            "contract.files must be an object of {name: content}",
        ));
    };
    if files.len() > 1 {
        return Err(Reject::Violation(ONE_FILE.into()));
    }
    let mut out = Vec::new();
    for (name, content) in files {
        let Some(content) = content.as_str() else {
            return Err(Reject::Malformed(format!(
                "contract.files.{name} must be a string"
            )));
        };
        out.push((name.clone(), content.to_string()));
    }
    if out.is_empty() {
        return Err(malformed("contract.files must contain at least one file"));
    }
    for (_, source) in &out {
        for token in ["import", "export"] {
            if has_word(source, token) {
                return Err(Reject::Violation(format!(
                    "guest files must be script-style: source contains \"{token}\" (no import/export statements)"
                )));
            }
        }
        if source.len() > MAX_GUEST_SOURCE_BYTES {
            return Err(Reject::Violation(format!(
                "guest source is {} bytes, over the {MAX_GUEST_SOURCE_BYTES} byte limit",
                source.len()
            )));
        }
    }
    if out.len() != 1 {
        return Err(Reject::Violation(ONE_FILE.into()));
    }
    Ok(out)
}

/// `file:line:col TSxxxx: message`, one per line, under the type-error heading.
pub fn diagnostics_feedback(diagnostics: &[Diagnostic]) -> String {
    let lines: Vec<String> = diagnostics
        .iter()
        .map(|d| {
            format!(
                "{}:{}:{} TS{}: {}",
                d.file, d.line, d.column, d.code, d.message
            )
        })
        .collect();
    format!("type errors — {FIX_AGAIN}:\n{}", lines.join("\n"))
}

const NO_TESTS: &str =
    "no test() calls found; add test(\"name\", () => { expect(...) }) calls per the spec";

/// What the model is told when the tests did not all pass: failed tests, guest logs, then the
/// sandbox error, in the order of the prototype's `formatSandboxFeedback`.
pub fn sandbox_feedback(run: &RunOutput) -> String {
    let crashed = run.status == "error" || run.status == "timeout";
    if run.tests.is_empty() && !crashed {
        return format!("{NO_TESTS}, and return the FULL file(s) again");
    }
    let mut lines = Vec::new();
    for t in run.tests.iter().filter(|t| !t.passed) {
        lines.push(match &t.error {
            Some(e) => format!("FAILED test \"{}\": {e}", t.name),
            None => format!("FAILED test \"{}\"", t.name),
        });
    }
    lines.extend(run.logs.iter().map(|l| format!("guest {l}")));
    if crashed {
        lines.push(match &run.error {
            Some(e) => format!("sandbox {}: {e}", run.status),
            None => format!("sandbox {}", run.status),
        });
        // An rs3 addition: the prototype only says a run with no tests found none.
        if run.tests.is_empty() {
            lines.push(NO_TESTS.into());
        }
    }
    lines.push(FIX_AGAIN.into());
    lines.join("\n")
}

/// Whether the tests ran and every one passed.
fn passes(run: &RunOutput) -> bool {
    run.status == "passed" && !run.tests.is_empty()
}

/// What the loop does next, derived from the committed records alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Ask the model: the exact prompt of this iteration's attempt.
    Call {
        iteration: u32,
        attempt: u32,
        prompt: String,
    },
    /// A model call is open: commit more of its reply.
    Generate,
    /// Type-check the reply's files.
    Check(Vec<(String, String)>),
    /// Run the JavaScript of a clean check.
    Run(String),
    /// Write the end record.
    End(Outcome),
    /// The stream has ended.
    Done,
}

/// One pass of the loop as a client sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Iteration {
    pub prompt: String,
    pub attempts: u32,
    /// The last attempt's reply bytes.
    pub reply: Vec<u8>,
    /// Every attempt's reply bytes, in order; the last equals `reply`.
    pub replies: Vec<Vec<u8>>,
    /// The check is committed (an empty `diagnostics` then means clean).
    pub checked: bool,
    /// The run's `(status, error)` once committed.
    pub run: Option<(String, Option<String>)>,
    pub files: Vec<(String, String)>,
    pub diagnostics: Vec<Diagnostic>,
    pub tests: Vec<crate::sandbox::TestResult>,
    pub logs: Vec<String>,
    /// What the model is told next; `None` when nothing went wrong.
    pub feedback: Option<String>,
}

/// The loop's state, folded from a coding stream.
#[derive(Debug, Clone)]
pub struct Loop {
    task: String,
    spec: String,
    max_iterations: u32,
    budget: u64,
    pub iterations: Vec<Iteration>,
    /// Takes what a stream with no call begun would write to its current iteration.
    stray: Iteration,
    /// Model calls begun so far; the k-th call gets the k-th canned reply.
    pub calls: usize,
    /// Tokens of the open call.
    pub open: usize,
    pub next: Next,
}

impl Loop {
    fn last(&mut self) -> &mut Iteration {
        self.iterations.last_mut().unwrap_or(&mut self.stray)
    }

    fn feedback(&self) -> Vec<String> {
        let notes = self.iterations.iter().filter_map(|i| i.feedback.clone());
        notes.collect()
    }

    fn call(&self, iteration: u32, attempt: u32) -> Next {
        let prompt = prompt(&self.task, &self.spec, &self.feedback(), attempt > 0);
        Next::Call {
            iteration,
            attempt,
            prompt,
        }
    }

    /// After a failed iteration: the next iteration, or the end.
    fn advance(&mut self, feedback: String) {
        self.last().feedback = Some(feedback);
        let done = self.iterations.len() as u32;
        self.next = if done >= self.max_iterations {
            Next::End(Outcome::MaxIterations)
        } else {
            self.call(done + 1, 0)
        };
    }

    fn reply_ended(&mut self) {
        let attempt = self.last().attempts - 1;
        let iteration = self.iterations.len() as u32;
        let reply = String::from_utf8_lossy(&self.last().reply).into_owned();
        match extract(&reply) {
            Ok(files) => {
                self.last().files = files.clone();
                self.next = Next::Check(files);
            }
            Err(Reject::Malformed(_)) if attempt > 0 => {
                self.next = Next::End(Outcome::LlmMalformed)
            }
            Err(Reject::Malformed(_)) => self.next = self.call(iteration, attempt + 1),
            Err(violation) => self.advance(violation.feedback()),
        }
    }

    fn checked(&mut self, diagnostics: &[Diagnostic], js: &Option<String>) {
        self.last().checked = true;
        if diagnostics.is_empty() {
            self.next = Next::Run(js.clone().unwrap_or_default());
        } else {
            self.last().diagnostics = diagnostics.to_vec();
            self.advance(diagnostics_feedback(diagnostics));
        }
    }

    fn ran(&mut self, run: &RunOutput) {
        let iteration = self.last();
        iteration.run = Some((run.status.clone(), run.error.clone()));
        iteration.tests = run.tests.clone();
        iteration.logs = run.logs.clone();
        if passes(run) {
            self.next = Next::End(Outcome::Success);
        } else {
            self.advance(sandbox_feedback(run));
        }
    }

    fn apply(&mut self, record: &Record) {
        match record {
            Record::ReplyStart {
                iteration,
                attempt,
                prompt,
                ..
            } => {
                if *attempt == 0 {
                    self.iterations.push(Iteration::default());
                    debug_assert_eq!(self.iterations.len() as u32, *iteration);
                }
                let last = self.last();
                last.prompt.clone_from(prompt);
                last.attempts = attempt + 1;
                last.reply.clear();
                last.replies.push(Vec::new());
                self.calls += 1;
                self.open = 0;
                self.next = Next::Generate;
            }
            Record::Tokens { tokens, .. } => {
                let bytes = tokens.iter().map(|t| *t as u8);
                let last = self.last();
                last.reply.extend(bytes.clone());
                if let Some(current) = last.replies.last_mut() {
                    current.extend(bytes);
                }
                self.open += tokens.len();
            }
            Record::ReplyEnd { .. } => self.reply_ended(),
            Record::Check { diagnostics, js } => self.checked(diagnostics, js),
            Record::Run(run) => self.ran(run),
            Record::CodeEnd { .. } | Record::Cancelled => self.next = Next::Done,
            _ => {}
        }
    }
}

impl Loop {
    /// The records of the next step that needs no checker or sandbox: a model call, a batch of
    /// its reply (cut at the budget), or the end. `None` for a check or a finished stream.
    pub fn step(&self, replies: &[String], batch: usize) -> Option<Vec<Record>> {
        let end = |outcome: Outcome| {
            Some(vec![Record::CodeEnd {
                outcome: outcome.as_str().into(),
            }])
        };
        match &self.next {
            Next::Call { .. } if self.calls >= replies.len() => end(Outcome::LlmError),
            Next::Call {
                iteration,
                attempt,
                prompt,
            } => Some(vec![Record::ReplyStart {
                iteration: *iteration,
                attempt: *attempt,
                prompt: prompt.clone(),
                prompt_tokens: Vec::new(),
            }]),
            Next::Generate => Some(self.reply_batch(&replies[self.calls - 1], batch)),
            Next::End(outcome) => end(*outcome),
            Next::Check(_) | Next::Run(_) | Next::Done => None,
        }
    }

    fn reply_batch(&self, reply: &str, batch: usize) -> Vec<Record> {
        let room = (self.budget as usize).saturating_sub(self.open);
        let (tokens, whole) = replay_batch(reply, self.open, room.min(batch));
        let ends = whole || tokens.len() == room;
        let mut records = vec![Record::Tokens {
            tokens,
            rng_state: Vec::new(),
        }];
        records.extend(ends.then(|| Record::ReplyEnd {
            reason: if whole { "eos" } else { "budget" }.into(),
        }));
        records
    }
}

/// Fold the records of a coding stream, or `None` when it does not start with `CodeStart`.
pub fn fold(records: &[Record]) -> Option<Loop> {
    let Some(Record::CodeStart {
        task,
        spec,
        max_iterations,
        budget,
        ..
    }) = records.first()
    else {
        return None;
    };
    let mut state = Loop {
        task: task.clone(),
        spec: spec.clone(),
        max_iterations: *max_iterations,
        budget: *budget,
        iterations: Vec::new(),
        stray: Iteration::default(),
        calls: 0,
        open: 0,
        next: Next::Done,
    };
    state.next = state.call(1, 0);
    for record in &records[1..] {
        state.apply(record);
    }
    Some(state)
}

/// The iterations and outcome a client sees, from committed records only.
pub fn view(records: &[Record]) -> Option<Value> {
    let state = fold(records)?;
    let outcome = match records.last() {
        Some(Record::CodeEnd { outcome }) => Value::from(outcome.as_str()),
        _ => Value::Null,
    };
    let iterations: Vec<Value> = state.iterations.iter().map(iteration_json).collect();
    Some(serde_json::json!({ "outcome": outcome, "iterations": iterations }))
}

fn iteration_json(i: &Iteration) -> Value {
    let files: serde_json::Map<String, Value> = i
        .files
        .iter()
        .map(|(name, text)| (name.clone(), Value::from(text.as_str())))
        .collect();
    serde_json::json!({
        "prompt": i.prompt,
        "attempts": i.attempts,
        "reply": String::from_utf8_lossy(&i.reply),
        "files": files,
        "diagnostics": i.diagnostics,
        "tests": i.tests,
        "logs": i.logs,
        "feedback": i.feedback,
        "replies": i.replies.iter().map(|r| String::from_utf8_lossy(r)).collect::<Vec<_>>(),
        "checked": i.checked,
        "run": i.run.as_ref().map(|(status, error)| serde_json::json!({"status": status, "error": error})),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::TestResult;

    fn reply(file: &str, body: &str) -> String {
        serde_json::json!({"explanation": "x", "files": {file: body}}).to_string()
    }

    #[test]
    fn fenced_json_extracts() {
        let text = format!("```json\n{}\n```", reply("a.ts", "const x = 1;"));
        assert_eq!(
            extract(&text).unwrap(),
            vec![("a.ts".into(), "const x = 1;".into())]
        );
    }

    #[test]
    fn unclosed_fence_and_prose_wrapped_json_are_salvaged() {
        let unclosed = format!("```\n{}", reply("a.ts", "1"));
        assert_eq!(extract(&unclosed).unwrap().len(), 1);
        let prose = format!("Here is the fix: {} hope it helps", reply("a.ts", "1"));
        assert_eq!(extract(&prose).unwrap().len(), 1);
    }

    #[test]
    fn unusable_replies_are_malformed() {
        let cases = [
            "no braces at all",
            "} backwards {",
            "{ not json }",
            "[1]",
            r#"{"explanation":"x"}"#,
            r#"{"files":{"a.ts":1}}"#,
            r#"{"files":{}}"#,
        ];
        for c in cases {
            assert!(matches!(extract(c), Err(Reject::Malformed(_))), "{c}");
        }
    }

    #[test]
    fn malformed_messages_match_the_prototype() {
        let m = |c| match extract(c) {
            Err(Reject::Malformed(m)) => m,
            other => panic!("{other:?}"),
        };
        assert_eq!(m("hello"), "response contains no JSON object");
        assert_eq!(m("} {"), "response contains no JSON object");
        assert_eq!(
            m("{ nope }"),
            "response is not valid JSON (fenced, bare, or brace-salvaged)"
        );
        assert_eq!(m("[1]"), "contract is not a JSON object");
        assert_eq!(
            m("{}"),
            "contract.files must be an object of {name: content}"
        );
        assert_eq!(
            m(r#"{"files":{"a":1}}"#),
            "contract.files.a must be a string"
        );
        assert_eq!(
            m(r#"{"files":{}}"#),
            "contract.files must contain at least one file"
        );
    }

    #[test]
    fn two_files_violate_the_single_script_rule() {
        let two = r#"{"files":{"a.ts":"1","b.ts":"2"}}"#;
        assert_eq!(
            extract(two).unwrap_err().feedback(),
            "return exactly ONE file (the sandbox executes a single script); fix and return the FULL file(s) again"
        );
    }

    #[test]
    fn import_and_export_are_violations_but_identifiers_are_fine() {
        for word in ["import", "export"] {
            let err = extract(&reply("a.ts", &format!("{word} x"))).unwrap_err();
            assert_eq!(
                err.feedback(),
                format!(
                    "guest files must be script-style: source contains \"{word}\" (no import/export statements); fix and return the FULL file(s) again"
                )
            );
        }
        for ok in ["const important = 1; reimport_x(); exports2", "my_export"] {
            assert!(extract(&reply("a.ts", ok)).is_ok(), "{ok}");
        }
        assert!(extract(&reply("a.ts", "1 /*export*/")).is_err());
    }

    #[test]
    fn source_size_limit_is_enforced() {
        let at = "a".repeat(MAX_GUEST_SOURCE_BYTES);
        assert!(extract(&reply("a.ts", &at)).is_ok());
        let over = "a".repeat(MAX_GUEST_SOURCE_BYTES + 1);
        assert!(matches!(
            extract(&reply("a.ts", &over)),
            Err(Reject::Violation(_))
        ));
    }

    #[test]
    fn diagnostics_feedback_lists_each_error() {
        let d = |line, message: &str| Diagnostic {
            file: "/a.ts".into(),
            line,
            column: 5,
            code: 2322,
            message: message.into(),
        };
        assert_eq!(
            diagnostics_feedback(&[d(1, "bad"), d(2, "worse")]),
            "type errors — fix and return the FULL file(s) again:\n/a.ts:1:5 TS2322: bad\n/a.ts:2:5 TS2322: worse"
        );
    }

    fn run(status: &str, tests: Vec<(&str, bool, Option<&str>)>, error: Option<&str>) -> RunOutput {
        RunOutput {
            status: status.into(),
            tests: tests
                .into_iter()
                .map(|(n, p, e)| TestResult {
                    name: n.into(),
                    passed: p,
                    error: e.map(Into::into),
                })
                .collect(),
            logs: vec!["log: hi".into()],
            error: error.map(Into::into),
        }
    }

    #[test]
    fn failed_test_feedback_names_the_test_error_and_logs() {
        let r = run(
            "failed",
            vec![
                ("ok", true, None),
                ("adds", false, Some("expected 3")),
                ("bare", false, None),
            ],
            None,
        );
        assert_eq!(
            sandbox_feedback(&r),
            "FAILED test \"adds\": expected 3\nFAILED test \"bare\"\nguest log: hi\nfix and return the FULL file(s) again"
        );
    }

    #[test]
    fn crashed_sandbox_feedback_leads_with_the_status() {
        let with_error = run("timeout", vec![], Some("took too long"));
        assert_eq!(
            sandbox_feedback(&with_error),
            format!(
                "guest log: hi\nsandbox timeout: took too long\n{NO_TESTS}\nfix and return the FULL file(s) again"
            )
        );
        let after_tests = run("error", vec![("t", true, None)], None);
        assert_eq!(
            sandbox_feedback(&after_tests),
            "guest log: hi\nsandbox error\nfix and return the FULL file(s) again"
        );
    }

    #[test]
    fn clean_run_without_tests_asks_for_tests() {
        assert_eq!(
            sandbox_feedback(&run("passed", vec![], None)),
            format!("{NO_TESTS}, and return the FULL file(s) again")
        );
    }

    #[test]
    fn the_prompt_carries_only_the_last_three_feedback_messages() {
        let fb: Vec<String> = (1..=5).map(|i| format!("fb{i}")).collect();
        let p = prompt("task", "spec", &fb, false);
        assert!(
            p.contains(SYSTEM_PROMPT)
                && p.contains("task\n\nSPEC:\nspec\n\nReturn ONLY the JSON contract.")
        );
        assert!(!p.contains("fb1") && !p.contains("fb2"));
        assert!(p.find("fb3") < p.find("fb4") && p.find("fb4") < p.find("fb5"));
        assert!(!p.contains(CORRECTIVE_PROMPT));
        assert!(prompt("task", "spec", &fb, true).contains(CORRECTIVE_PROMPT));
        assert_eq!(feedback_window(&fb[..2]).len(), 2);
    }

    #[test]
    fn outcome_names() {
        let names: Vec<_> = [
            Outcome::Success,
            Outcome::MaxIterations,
            Outcome::LlmError,
            Outcome::LlmMalformed,
        ]
        .map(Outcome::as_str)
        .to_vec();
        assert_eq!(
            names,
            ["success", "max_iterations", "llm_error", "llm_malformed"]
        );
    }
}
