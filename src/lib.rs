#![forbid(unsafe_code)]
//! rs3: a small key/value information object store served over HTTP, with an
//! embedded JavaScript runtime that can call back into the store.

pub mod config;
pub mod goals;
pub mod http;
pub mod journal;
pub mod js;
pub mod lease;
pub mod model;
pub mod raft;
pub mod state;
pub mod storage;
pub mod store;
