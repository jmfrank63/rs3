#![forbid(unsafe_code)]
//! rs3: a small key/value information object store served over HTTP, with an
//! embedded JavaScript runtime that can call back into the store.

pub mod config;
pub mod http;
pub mod js;
