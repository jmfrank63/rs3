use super::MAP;
use deno_core::serde_json::Value;

pub fn list() -> String {
    let guard = MAP.guard();
    let mut out = String::new();
    out.push_str("[");
    for (k, v) in MAP.iter(&guard) {
        out.push_str(format!("{{\"{}\" : {}}}", k, v).as_str());
        out.push_str(",");
    }
    out.push_str("]");
    out
}

pub fn insert(request_body: String) -> Value {
    let guard = MAP.guard();
    let entry: serde_json::Value = serde_json::from_str(request_body.as_str()).unwrap();
    let obj = entry.as_object().unwrap();
    for (k, v) in obj.iter() {
        MAP.insert(k.to_string(), v.to_string(), &guard);
    }
    for (k, v) in MAP.iter(&guard) {
        println!("{} {}", k, v);
    }
    entry
}

pub fn delete(path: String) -> String {
    let guard = MAP.guard();
    let entry = MAP.remove(path.as_str(), &guard).unwrap();
    let entry = entry.to_owned();
    entry
}

pub fn get(path: String) -> String {
    let guard = MAP.guard();
    let entry = MAP.get(path.as_str(), &guard).unwrap();
    let entry = entry.to_owned();
    entry
}
