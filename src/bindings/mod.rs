//! Handlers which access the hash_map
//!
//! The handlers will be either called by the service handlers
//! or by the JsRuntime of Deno directly (bindings)
use super::MAP;
use deno_core::{op_sync, JsRuntime, RuntimeOptions};
use rusty_v8::{Context, ContextScope, HandleScope};

pub fn list() -> String {
    let guard = MAP.guard();
    let mut out = String::new();
    out.push_str("[");
    for (k, v) in MAP.iter(&guard) {
        out.push_str(format!("{{\"{}\" : \"{}\"}}", k, v).as_str());
        out.push_str(",");
    }
    if out.ends_with(",") {
        out = out.strip_suffix(",").unwrap().to_owned();
    }
    out.push_str("]");
    out
}

pub fn insert(request_body: String) -> String {
    let guard = MAP.guard();
    let entry: serde_json::Value = serde_json::from_str(request_body.as_str()).unwrap();
    let obj = entry.as_object().unwrap();
    for (k, v) in obj.iter() {
        let v: String = serde_json::from_value(v.to_owned()).unwrap();
        MAP.insert(k.to_string(), v.to_string(), &guard);
    }
    entry.to_string()
}

pub fn delete(key: String) -> String {
    let guard = MAP.guard();
    let entry = MAP.remove(key.as_str(), &guard).unwrap();
    let entry = entry.to_owned();
    entry
}

pub fn get(key: String) -> String {
    let guard = MAP.guard();
    if MAP.is_empty() {
        "[]".to_string()
    } else {
        let value = MAP.get(key.as_str(), &guard).unwrap();
        value.to_owned().to_string()
    }
}

// Takes a vaild JavaScript code as a string and executes it
pub fn patch(key: String) -> String {
    let value = get(key.clone());
    let mut runtime = interpreter();
    let result = runtime
        .execute_script(key.as_str(), value.as_str())
        .unwrap();
    {
        let isolate = runtime.v8_isolate();
        let scope = &mut HandleScope::new(isolate);
        let context = Context::new(scope);
        let scope = &mut ContextScope::new(scope, context);
        let res = result.get(scope);
        res.to_rust_string_lossy(scope)
    }
}

// Creates the V8 engine JavaScript runtime
// And registers the handlers for use by the engine
pub fn interpreter() -> JsRuntime {
    let mut runtime = JsRuntime::new(RuntimeOptions {
        ..Default::default()
    });
    runtime.register_op(
        "rs3_list",
        op_sync(|_state, _data: String, _: ()| Ok(list())),
    );
    runtime.register_op(
        "rs3_get",
        op_sync(|_state, key: String, _: ()| Ok(get(key))),
    );
    runtime.register_op(
        "rs3_insert",
        op_sync(|_state, request_body: String, _: ()| Ok(insert(request_body))),
    );
    runtime.register_op(
        "rs3_delete",
        op_sync(|_state, key: String, _: ()| Ok(delete(key))),
    );
    runtime.register_op(
        "rs3_patch",
        op_sync(|_state, key: String, _: ()| Ok(patch(key))),
    );
    runtime.sync_ops_cache();
    runtime
}
