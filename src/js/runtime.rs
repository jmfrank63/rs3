//! Runs one script on a fresh `JsRuntime` and stringifies its completion value.
//! This module is free of actix-web.
use deno_core::error::JsError;
use deno_core::{JsRuntime, RuntimeOptions, v8};

use super::ExecError;
use super::ops::{ExecCtx, rs3_ext};

/// Run `source` as a script named `name` with the store ops bound to `ctx`, and
/// return `String(completion value)`.
///
/// Every call creates one fresh runtime and drops it before returning, so no
/// state leaks between scripts. It must be called on a thread with a tokio
/// runtime entered: deno_core aborts when V8 posts a delayed task without one.
/// A thrown exception, a syntax error, an uncaught op error or an exception
/// while converting the completion value to a string becomes
/// `ExecError::Script` carrying the JS exception message; the stack goes to
/// stderr only.
pub fn run_script(ctx: ExecCtx, name: &str, source: String) -> Result<String, ExecError> {
    let mut rt = JsRuntime::try_new(RuntimeOptions {
        extensions: vec![rs3_ext::init(ctx)],
        ..Default::default()
    })
    .map_err(|e| ExecError::Internal(format!("cannot create JS runtime: {e}")))?;
    let global = rt.execute_script(name.to_owned(), source).map_err(|e| {
        // Debug-format the name so a key with control characters cannot forge log lines.
        eprintln!("js error in {name:?}: {e}");
        ExecError::Script {
            message: e.exception_message.clone(),
        }
    })?;
    // Read the value in the runtime's own main context; never build a new one.
    deno_core::scope!(scope, &mut rt);
    let local = v8::Local::new(scope, global);
    stringify(scope, local).map_err(|e| {
        eprintln!("js error in {name:?} while converting the result to a string: {e}");
        ExecError::Script {
            message: e.exception_message.clone(),
        }
    })
}

/// `String(value)` with JS semantics. A symbol becomes `Symbol(description)`,
/// as `String(..)` does. Any other value goes through `ToString`, which can run
/// user code (`toString`, `valueOf`, `Symbol.toPrimitive`, Proxy traps); an
/// exception there is caught and returned instead of being swallowed.
fn stringify<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Result<String, Box<JsError>> {
    if let Ok(sym) = v8::Local::<v8::Symbol>::try_from(value) {
        let desc = sym.description(scope);
        // A description is a string or undefined, so this conversion cannot throw.
        let desc = if desc.is_undefined() {
            String::new()
        } else {
            desc.to_rust_string_lossy(scope)
        };
        return Ok(format!("Symbol({desc})"));
    }
    v8::tc_scope!(let tc, scope);
    if let Some(s) = value.to_string(tc) {
        return Ok(s.to_rust_string_lossy(tc));
    }
    let exception = tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
    Err(JsError::from_v8_exception(tc, exception))
}

#[cfg(test)]
mod tests {
    use crate::js::{ExecError, block_on, run_js, test_executor};
    use crate::store::Store;

    fn script_err(m: &str) -> Result<String, ExecError> {
        Err(ExecError::Script {
            message: m.to_owned(),
        })
    }

    #[test]
    fn completion_values_are_stringified() {
        let cases = [
            ("6*20", "120"),
            ("let a=1;", "undefined"),
            ("({a:1})", "[object Object]"),
            ("Promise.resolve(1)", "[object Promise]"),
            ("null", "null"),
            ("'hi'", "hi"),
            ("''", ""),
            ("", "undefined"),
        ];
        for (src, want) in cases {
            assert_eq!(
                run_js(&Store::new(), src),
                Ok(want.to_owned()),
                "source {src:?}"
            );
        }
    }

    #[test]
    fn symbol_completion_values_match_js_string() {
        let cases = [
            ("Symbol('x')", "Symbol(x)"),
            ("Symbol()", "Symbol()"),
            ("Symbol('')", "Symbol()"),
            ("Symbol.iterator", "Symbol(Symbol.iterator)"),
        ];
        for (src, want) in cases {
            assert_eq!(
                run_js(&Store::new(), src),
                Ok(want.to_owned()),
                "source {src:?}"
            );
        }
    }

    #[test]
    fn exception_while_stringifying_is_a_script_error() {
        assert_eq!(
            run_js(&Store::new(), "({ toString() { throw new Error('x') } })"),
            script_err("Uncaught Error: x")
        );
        for src in [
            "Object.create(null)",
            "({ [Symbol.toPrimitive]() { return Symbol() } })",
            "Object(Symbol('x'))",
        ] {
            match run_js(&Store::new(), src) {
                Err(ExecError::Script { message }) => {
                    assert!(
                        message.starts_with("Uncaught TypeError"),
                        "{src}: {message}"
                    )
                }
                other => panic!("{src}: expected a script error, got {other:?}"),
            }
        }
    }

    #[test]
    fn stringifying_runs_user_code_whose_writes_stay() {
        let store = Store::new();
        let src =
            r#"({ toString() { Deno.core.ops.op_rs3_insert('{"seen":"1"}'); return 'ok' } })"#;
        assert_eq!(run_js(&store, src), Ok("ok".to_owned()));
        assert_eq!(store.get("seen"), Ok("1".to_owned()));
    }

    #[test]
    fn thrown_errors_become_script_errors() {
        let store = Store::new();
        assert_eq!(
            run_js(&store, "throw new Error('boom')"),
            script_err("Uncaught Error: boom")
        );
        match run_js(&store, "let = ;") {
            Err(ExecError::Script { message }) => {
                assert!(message.starts_with("Uncaught SyntaxError"), "{message}")
            }
            other => panic!("expected a syntax error, got {other:?}"),
        }
        assert_eq!(
            run_js(&store, "nope"),
            script_err("Uncaught ReferenceError: nope is not defined")
        );
        assert_eq!(
            run_js(&store, "throw 'plain'"),
            script_err("Uncaught plain")
        );
    }

    #[test]
    fn error_messages_keep_non_ascii() {
        assert_eq!(
            run_js(&Store::new(), "throw new Error('ключ ✓')"),
            script_err("Uncaught Error: ключ ✓")
        );
    }

    #[test]
    fn non_ascii_script_name_runs() {
        assert_eq!(
            block_on(test_executor().execute(Store::new(), "ключ/🔑".into(), "1+1".into())),
            Ok("2".to_owned())
        );
    }

    #[test]
    fn deep_js_recursion_is_a_catchable_range_error() {
        let src = "function f(n) { return f(n + 1) + 1; } \
                   try { f(0); 'no throw' } catch (e) { e instanceof RangeError }";
        assert_eq!(run_js(&Store::new(), src), Ok("true".to_owned()));
    }
}
