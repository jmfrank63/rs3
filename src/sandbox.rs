//! Runs a guest's emitted JavaScript on a fresh runtime with no ops: no store, no network, no
//! clock beyond V8's own. The guest sees `test`, `expect`, `assert` and a capturing `console`; the
//! results live in closures the guest cannot reach, so it cannot forge them. Status names and
//! error texts match the iso-agents prototype because the loop's feedback compares against them.

use std::sync::mpsc;
use std::time::Duration;

use deno_core::{JsRuntime, RuntimeOptions, v8};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOutput {
    /// `passed`, `failed`, `error` or `timeout`.
    pub status: String,
    pub tests: Vec<TestResult>,
    pub logs: Vec<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct Report {
    tests: Vec<TestResult>,
    logs: Vec<String>,
}

/// Defines the locked `test`, `expect`, `assert` and `console`, and evaluates to a function that
/// returns the private records as JSON. Console retention is capped at 8192 bytes.
const PRELUDE: &str = r#"(function (g) {
  var records = [], logs = [], used = 0, CAP = 8192;
  function isNaNValue(v) { return typeof v === "number" && v !== v; }
  function deepEqual(a, b) {
    if (isNaNValue(a) || isNaNValue(b)) return false;
    if (a === b) return true;
    if (a === null || b === null) return false;
    if (typeof a !== typeof b || typeof a !== "object") return false;
    var arr = Array.isArray(a);
    if (arr !== Array.isArray(b)) return false;
    if (arr) {
      if (a.length !== b.length) return false;
      for (var i = 0; i < a.length; i++) if (!deepEqual(a[i], b[i])) return false;
      return true;
    }
    var ka = Object.keys(a), kb = Object.keys(b);
    if (ka.length !== kb.length) return false;
    for (var j = 0; j < ka.length; j++) {
      var k = ka[j];
      if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
      if (!deepEqual(a[k], b[k])) return false;
    }
    return true;
  }
  function shown(v) {
    if (typeof v === "number" && !isFinite(v)) return String(v);
    try { var t = JSON.stringify(v); return t === undefined ? String(v) : t; } catch (e) { return String(v); }
  }
  function testFn(name, fn) {
    try { fn(); records.push({ name: String(name), passed: true, error: null }); }
    catch (e) {
      records.push({ name: String(name), passed: false,
        error: e && e.name !== undefined ? String(e.name) + ": " + String(e.message) : String(e) });
    }
  }
  function expectFn(actual) {
    return {
      toEqual: function (expected) {
        if (!deepEqual(actual, expected)) throw new Error("expected " + shown(expected) + ", got " + shown(actual));
      },
      toBeTrue: function (msg) {
        if (actual !== true) throw new Error(msg === undefined ? "expected true, got " + shown(actual) : msg);
      },
      toBeCloseTo: function (expected, eps) {
        var tol = eps === undefined ? 1e-6 : eps;
        if (typeof actual !== "number" || !isFinite(actual) || typeof expected !== "number" || !isFinite(expected))
          throw new Error("toBeCloseTo needs finite numbers, got " + shown(actual) + " vs " + shown(expected));
        if (typeof tol !== "number" || !isFinite(tol) || tol < 0)
          throw new Error("toBeCloseTo tolerance must be a non-negative finite number, got " + shown(tol));
        if (Math.abs(actual - expected) > tol) throw new Error("expected ~" + expected + ", got " + actual);
      },
    };
  }
  function assertFn(cond, msg) {
    if (!cond) throw new Error(msg === undefined ? "assertion failed" : msg);
  }
  function lock(name, value) {
    Object.defineProperty(g, name, { value: value, writable: false, enumerable: true, configurable: false });
  }
  lock("test", testFn);
  lock("expect", expectFn);
  lock("assert", assertFn);
  function show(v) { return typeof v === "string" ? v : shown(v); }
  function log() {
    var room = CAP - used;
    if (room <= 0) return;
    var parts = [];
    for (var i = 0; i < arguments.length; i++) parts.push(show(arguments[i]));
    var line = parts.join(" ").slice(0, room - 1);
    used += line.length + 1;
    logs.push(line);
  }
  var c = {};
  ["log", "info", "warn", "error", "debug"].forEach(function (l) { c[l] = log; });
  lock("console", c);
  return function () { return JSON.stringify({ tests: records, logs: logs }); };
})(globalThis)"#;

/// Run `js` on a fresh runtime, aborting it after `timeout`. `seed` is accepted now so callers
/// need not change when determinism lands; nothing reads it yet.
pub fn run(js: &str, _seed: u64, timeout: Duration) -> RunOutput {
    let js = js.to_owned();
    std::thread::Builder::new()
        .name("sandbox".into())
        .spawn(move || run_here(&js, timeout))
        .and_then(|h| {
            h.join()
                .map_err(|_| std::io::Error::other("sandbox panicked"))
        })
        .unwrap_or_else(|e| failure("error", format!("sandbox thread: {e}")))
}

fn failure(status: &str, error: String) -> RunOutput {
    RunOutput {
        status: status.into(),
        tests: vec![],
        logs: vec![],
        error: Some(error),
    }
}

fn run_here(js: &str, timeout: Duration) -> RunOutput {
    // deno_core aborts when V8 posts a delayed task and no tokio runtime is entered.
    let Ok(tokio_rt) = tokio::runtime::Builder::new_current_thread().build() else {
        return failure("error", "cannot start the sandbox executor".into());
    };
    let _enter = tokio_rt.enter();
    let mut rt = match JsRuntime::try_new(RuntimeOptions::default()) {
        Ok(rt) => rt,
        Err(e) => return failure("error", format!("cannot create JS runtime: {e}")),
    };
    let handle = rt.v8_isolate().thread_safe_handle();
    let (done, wait) = mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        // Only a silent sender (the run is still going) means the deadline passed.
        let expired = matches!(
            wait.recv_timeout(timeout),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        if expired {
            handle.terminate_execution();
        }
        expired
    });
    let mut out = execute(&mut rt, js);
    drop(done);
    if watchdog.join().unwrap_or(false) {
        out = failure(
            "timeout",
            format!(
                "execution exceeded the {}ms sandbox deadline",
                timeout.as_millis()
            ),
        );
    }
    out
}

fn execute(rt: &mut JsRuntime, js: &str) -> RunOutput {
    let report = match rt.execute_script("harness.js", PRELUDE) {
        Ok(g) => g,
        Err(e) => return failure("error", format!("harness setup failed: {e}")),
    };
    // Strict like the prototype; no newline so the guest's line numbers stay put.
    let guest = rt.execute_script("guest.js", format!("\"use strict\";{js}"));
    let error = guest.err().map(|e| match (&e.name, &e.message) {
        (Some(n), Some(m)) => format!("{n}: {m}"),
        _ => e.exception_message.clone(),
    });
    let Some(Report { tests, logs }) = read_report(rt, report) else {
        return failure("error", "harness report unreadable".into());
    };
    let status = match (&error, tests.iter().all(|t| t.passed)) {
        (Some(_), _) => "error",
        (None, true) => "passed",
        (None, false) => "failed",
    };
    RunOutput {
        status: status.into(),
        tests,
        logs,
        error,
    }
}

fn read_report(rt: &mut JsRuntime, report: deno_core::v8::Global<v8::Value>) -> Option<Report> {
    deno_core::scope!(scope, rt);
    let func = v8::Local::<v8::Function>::try_from(v8::Local::new(scope, report)).ok()?;
    let recv = v8::undefined(scope).into();
    let json = func.call(scope, recv, &[])?.to_rust_string_lossy(scope);
    serde_json::from_str(&json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Duration = Duration::from_secs(5);

    fn run_js(js: &str) -> RunOutput {
        run(js, 0, T)
    }

    #[test]
    fn passing_tests_are_reported_by_name() {
        let out =
            run_js("test('adds', () => expect(1 + 1).toEqual(2)); test('t', () => assert(true));");
        assert_eq!(out.status, "passed");
        let names: Vec<_> = out
            .tests
            .iter()
            .map(|t| (t.name.as_str(), t.passed))
            .collect();
        assert_eq!(names, [("adds", true), ("t", true)]);
        assert_eq!(out.error, None);
    }

    #[test]
    fn a_failing_expect_names_the_test_and_the_difference() {
        let out = run_js("test('ok', () => {}); test('bad', () => expect(1 + 1).toEqual(3));");
        assert_eq!(out.status, "failed");
        assert_eq!(
            out.tests[1].error.as_deref(),
            Some("Error: expected 3, got 2")
        );
        assert!(out.tests[0].passed);
    }

    #[test]
    fn matchers_judge_like_the_prototype() {
        let src = r#"
          test('deep', () => expect({a: [1, {b: 2}]}).toEqual({a: [1, {b: 2}]}));
          test('nan', () => expect(NaN).toEqual(NaN));
          test('arr', () => expect([1]).toEqual({0: 1}));
          test('len', () => expect([1, 2]).toEqual([1]));
          test('keys', () => expect({a: 1}).toEqual({b: 1}));
          test('null', () => expect(null).toEqual({}));
          test('true', () => expect(1).toBeTrue());
          test('true msg', () => expect(1).toBeTrue('custom'));
          test('close', () => expect(0.1 + 0.2).toBeCloseTo(0.3));
          test('far', () => expect(1).toBeCloseTo(2));
          test('far eps', () => expect(1).toBeCloseTo(2, 5));
          test('nan close', () => expect(NaN).toBeCloseTo(1));
          test('bad eps', () => expect(1).toBeCloseTo(1, -1));
          test('assert', () => assert(false));
          test('assert msg', () => assert(0, 'nope'));
          test('inf', () => expect(Infinity).toEqual(1));
          test('throws string', () => { throw 'plain'; });
        "#;
        let out = run_js(src);
        let got: Vec<_> = out
            .tests
            .iter()
            .map(|t| {
                (
                    t.name.as_str(),
                    t.passed,
                    t.error.clone().unwrap_or_default(),
                )
            })
            .collect();
        let want: Vec<(&str, bool, &str)> = vec![
            ("deep", true, ""),
            ("nan", false, "Error: expected NaN, got NaN"),
            ("arr", false, "Error: expected {\"0\":1}, got [1]"),
            ("len", false, "Error: expected [1], got [1,2]"),
            ("keys", false, "Error: expected {\"b\":1}, got {\"a\":1}"),
            ("null", false, "Error: expected {}, got null"),
            ("true", false, "Error: expected true, got 1"),
            ("true msg", false, "Error: custom"),
            ("close", true, ""),
            ("far", false, "Error: expected ~2, got 1"),
            ("far eps", true, ""),
            (
                "nan close",
                false,
                "Error: toBeCloseTo needs finite numbers, got NaN vs 1",
            ),
            (
                "bad eps",
                false,
                "Error: toBeCloseTo tolerance must be a non-negative finite number, got -1",
            ),
            ("assert", false, "Error: assertion failed"),
            ("assert msg", false, "Error: nope"),
            ("inf", false, "Error: expected 1, got Infinity"),
            ("throws string", false, "plain"),
        ];
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(&want) {
            assert_eq!((g.0, g.1, g.2.as_str()), *w);
        }
    }

    #[test]
    fn close_numbers_pass_within_tolerance() {
        let out = run_js("test('c', () => expect(1).toBeCloseTo(1.0000001));");
        assert_eq!(out.status, "passed");
    }

    #[test]
    fn a_top_level_throw_is_an_error_but_keeps_earlier_tests() {
        let out = run_js("test('first', () => {}); throw new TypeError('boom');");
        assert_eq!(out.status, "error");
        assert_eq!(out.error.as_deref(), Some("TypeError: boom"));
        assert_eq!(out.tests.len(), 1);
    }

    #[test]
    fn a_syntax_error_is_an_error() {
        let out = run_js("let = ;");
        assert_eq!(out.status, "error");
        assert!(out.error.unwrap().starts_with("SyntaxError"));
    }

    #[test]
    fn console_output_is_captured_in_order() {
        let out = run_js("console.log('a', 1, {x: 2}); console.error('b'); console.warn('c');");
        assert_eq!(out.logs, ["a 1 {\"x\":2}", "b", "c"]);
        assert_eq!(out.status, "passed");
    }

    #[test]
    fn console_retention_is_capped() {
        let out = run_js("console.log('x'.repeat(100000)); console.log('late');");
        assert_eq!(out.logs.len(), 1);
        assert_eq!(out.logs[0].len(), 8191);
        let out = run_js("for (let i = 0; i < 5000; i++) console.log('0123456789');");
        assert_eq!(out.logs.len(), 745);
        assert_eq!(out.logs[743].len(), 10);
        assert_eq!(out.logs[744].len(), 7);
        let out = run_js("console.log('x'.repeat(8190)); console.log('abcdef');");
        assert_eq!(out.logs[1], "");
    }

    #[test]
    fn the_guest_cannot_replace_the_harness() {
        let out = run_js("test = () => {};");
        assert_eq!(out.status, "error");
        let out =
            run_js("try { globalThis.expect = 1 } catch (e) {} test('t', () => assert(false));");
        assert_eq!(out.status, "failed");
    }

    #[test]
    fn the_guest_has_no_host_access() {
        let out = run_js(
            "test('no ops', () => assert(typeof Deno === 'undefined' || typeof Deno.core === 'undefined' || !Deno.core.ops.op_rs3_insert));",
        );
        assert_eq!(out.status, "passed");
    }

    #[test]
    fn runs_do_not_share_state() {
        run_js("globalThis.leak = 1;");
        let out = run_js("test('clean', () => assert(typeof leak === 'undefined'));");
        assert_eq!(out.status, "passed");
    }

    #[test]
    fn an_endless_loop_times_out_within_the_deadline() {
        let start = std::time::Instant::now();
        let out = run("while (true) {}", 0, Duration::from_millis(200));
        assert_eq!(out.status, "timeout");
        assert!(out.error.unwrap().contains("200ms"));
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_fast_run_does_not_wait_for_the_deadline() {
        let start = std::time::Instant::now();
        let out = run("1", 0, Duration::from_secs(30));
        assert_eq!(out.status, "passed");
        assert!(start.elapsed() < Duration::from_secs(10));
    }
}
