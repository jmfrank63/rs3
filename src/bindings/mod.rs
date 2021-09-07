use actix_web::web::{Data, Path};
use deno_core::serde_json::Value;
use flurry::HashMap;

pub fn list(map: Data<HashMap<String, String>>) -> String {
    let guard = map.guard();
    let mut out = String::new();
    out.push_str("[");
    for (k, v) in map.iter(&guard) {
        out.push_str(format!("{{\"{}\" : {}}}", k, v).as_str());
        out.push_str(",");
    }
    out.push_str("]");
    out
}

pub fn insert(request_body: String, map: Data<HashMap<String, String>>) -> Value {
    let guard = map.guard();
    let entry: serde_json::Value = serde_json::from_str(request_body.as_str()).unwrap();
    let obj = entry.as_object().unwrap();
    for (k, v) in obj.iter() {
        map.insert(k.to_string(), v.to_string(), &guard);
    }
    for (k, v) in map.iter(&guard) {
        println!("{} {}", k, v);
    }
    entry
}

pub fn delete(path: Path<String>, map: Data<HashMap<String, String>>) -> String {
    let path = path.into_inner();
    let guard = map.guard();
    let entry = map.remove(path.as_str(), &guard).unwrap();
    let entry = entry.to_owned();
    entry
}

pub fn get(path: Path<String>, map: Data<HashMap<String, String>>) -> String {
    let path = path.into_inner();
    let guard = map.guard();
    let entry = map.get(path.as_str(), &guard).unwrap();
    let entry = entry.to_owned();
    entry
}
