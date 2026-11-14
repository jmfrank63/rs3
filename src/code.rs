//! The coding loop's text, ported byte for byte from iso-agents `loop.ts` and `extractor.ts`:
//! prompts, reply extraction and the feedback the model gets back.

use crate::checker::Diagnostic;
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
const FIX_AGAIN: &str = "fix and return the FULL file(s) again";

/// How a goal ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    MaxIterations,
    RunDeadline,
    LlmError,
    LlmMalformed,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Success => "success",
            Outcome::MaxIterations => "max_iterations",
            Outcome::RunDeadline => "run_deadline",
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
        return Err(Reject::Violation(
            "return exactly ONE file (the sandbox executes a single script)".into(),
        ));
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

/// What the model is told when the tests did not all pass.
pub fn sandbox_feedback(run: &RunOutput) -> String {
    let crashed = run.status == "error" || run.status == "timeout";
    let mut lines = Vec::new();
    if crashed {
        lines.push(match &run.error {
            Some(e) => format!("sandbox {}: {e}", run.status),
            None => format!("sandbox {}", run.status),
        });
        if run.tests.is_empty() {
            lines.push(NO_TESTS.into());
        }
    } else if run.tests.is_empty() {
        return format!("{NO_TESTS}, and return the FULL file(s) again");
    } else {
        for t in run.tests.iter().filter(|t| !t.passed) {
            lines.push(match &t.error {
                Some(e) => format!("FAILED test \"{}\": {e}", t.name),
                None => format!("FAILED test \"{}\"", t.name),
            });
        }
        lines.extend(run.logs.iter().map(|l| format!("guest log: {l}")));
    }
    lines.push(FIX_AGAIN.into());
    lines.join("\n")
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
            logs: vec!["hi".into()],
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
                "sandbox timeout: took too long\n{NO_TESTS}\nfix and return the FULL file(s) again"
            )
        );
        let after_tests = run("error", vec![("t", true, None)], None);
        assert_eq!(
            sandbox_feedback(&after_tests),
            "sandbox error\nfix and return the FULL file(s) again"
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
            Outcome::RunDeadline,
            Outcome::LlmError,
            Outcome::LlmMalformed,
        ]
        .map(Outcome::as_str)
        .to_vec();
        assert_eq!(
            names,
            [
                "success",
                "max_iterations",
                "run_deadline",
                "llm_error",
                "llm_malformed"
            ]
        );
    }
}
