//! Server configuration.
//!
//! Settings come from the dotenv file (looked up in the current directory, then
//! its ancestors) overlaid by the process environment, which wins. The process
//! environment is only read, never modified. A missing dotenv file is ignored;
//! any other dotenv problem (a line that does not parse, an unreadable file) is
//! a configuration error. Only the keys `SERVER.HOST`, `SERVER.PORT`,
//! `STORAGE.PATH`, `SCHEDULER.WORKERS`, `MODEL.TOKENS_PER_SECOND`, `MODEL.PATH`,
//! `MODEL.TOKENIZER`, `MODEL.THREADS` (default 4) and the `CLUSTER.*` keys
//! (case-insensitive) are used. `MODEL.PATH` and `MODEL.TOKENIZER` go together. Both server keys are required: there are no
//! defaults, and missing or invalid values are an error. Without `STORAGE.PATH`
//! the store keeps its data in memory only. `SCHEDULER.WORKERS` (default 2) and
//! `MODEL.TOKENS_PER_SECOND` (default 1000) must be positive integers.
use std::ffi::OsString;
use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::time::Duration;

use config::{Config, ConfigError, Environment};
use serde::Deserialize;

/// The only configuration keys read from the dotenv file and the environment.
const CONFIG_KEYS: [&str; 12] = [
    "server.host",
    "server.port",
    "storage.path",
    "scheduler.workers",
    "model.tokens_per_second",
    "model.path",
    "model.tokenizer",
    "model.threads",
    "cluster.node_id",
    "cluster.peers",
    "cluster.snapshot_every",
    "cluster.shrink_after",
];

/// Address the HTTP server binds to.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
}

/// Where the store keeps its data.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct StorageSettings {
    /// The database file. `None` keeps data in memory only.
    pub path: Option<PathBuf>,
}

/// How many goals run at once.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SchedulerSettings {
    pub workers: NonZeroUsize,
}

impl Default for SchedulerSettings {
    fn default() -> Self {
        Self {
            workers: NonZeroUsize::MIN.saturating_add(1),
        }
    }
}

/// How fast a worker emits tokens, and which real model it runs.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ModelSettings {
    pub tokens_per_second: NonZeroU32,
    /// The GGUF weights; with `tokenizer`, switches goals from the fake model to the real one.
    pub path: Option<PathBuf>,
    pub tokenizer: Option<PathBuf>,
    /// Threads the real model computes on.
    pub threads: NonZeroUsize,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            tokens_per_second: NonZeroU32::MIN.saturating_add(999),
            path: None,
            tokenizer: None,
            threads: NonZeroUsize::MIN.saturating_add(3),
        }
    }
}

/// Static cluster membership: this node's id and every member's id and address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterSettings {
    pub node_id: u64,
    pub peers: Vec<(u64, String)>,
    /// Log entries applied between snapshots.
    pub snapshot_every: u64,
    /// How long two peers may stay silent before the leader drops them from the voters.
    pub shrink_after: Duration,
}

/// Log entries applied between snapshots unless `CLUSTER.SNAPSHOT_EVERY` says otherwise.
const SNAPSHOT_EVERY: u64 = 10_000;

/// Seconds two peers may stay silent before the shrink unless `CLUSTER.SHRINK_AFTER` says otherwise.
/// A shrink is permanent and no timeout tells slow from dead, so the default is cautious.
const SHRINK_AFTER: u64 = 600;

/// The cluster keys as written, before they are checked against each other.
#[derive(Debug, Default, Deserialize)]
struct RawCluster {
    node_id: Option<u64>,
    peers: Option<String>,
    snapshot_every: Option<u64>,
    shrink_after: Option<u64>,
}

impl RawCluster {
    fn parse(self) -> Result<Option<ClusterSettings>, ConfigError> {
        let (node_id, peers) = match (self.node_id, self.peers) {
            (None, None) => return Ok(None),
            (Some(id), Some(peers)) => (id, peers),
            _ => {
                return Err(invalid(
                    "CLUSTER.NODE_ID and CLUSTER.PEERS must be set together",
                ));
            }
        };
        let peers = parse_peers(&peers)?;
        if !peers.iter().any(|(id, _)| *id == node_id) {
            return Err(invalid(format!(
                "CLUSTER.NODE_ID {node_id} is not in CLUSTER.PEERS"
            )));
        }
        if peers.len() % 2 == 0 {
            return Err(invalid("CLUSTER.PEERS must list an odd number of peers"));
        }
        let snapshot_every = self.snapshot_every.unwrap_or(SNAPSHOT_EVERY);
        if snapshot_every == 0 {
            return Err(invalid("CLUSTER.SNAPSHOT_EVERY must be positive"));
        }
        let shrink_after = self.shrink_after.unwrap_or(SHRINK_AFTER);
        if shrink_after == 0 {
            return Err(invalid("CLUSTER.SHRINK_AFTER must be positive"));
        }
        Ok(Some(ClusterSettings {
            node_id,
            peers,
            snapshot_every,
            shrink_after: Duration::from_secs(shrink_after),
        }))
    }
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::Message(message.into())
}

/// Parse `1=host:port,2=host:port,...`; ids must be unique.
fn parse_peers(text: &str) -> Result<Vec<(u64, String)>, ConfigError> {
    let mut peers: Vec<(u64, String)> = Vec::new();
    for pair in text.split(',') {
        let bad = || invalid(format!("CLUSTER.PEERS entry {pair:?} is not id=host:port"));
        let (id, addr) = pair.split_once('=').ok_or_else(bad)?;
        let id: u64 = id.trim().parse().map_err(|_| bad())?;
        let addr = addr.trim();
        if addr.is_empty() {
            return Err(bad());
        }
        if peers.iter().any(|(seen, _)| *seen == id) {
            return Err(invalid(format!("CLUSTER.PEERS repeats id {id}")));
        }
        peers.push((id, addr.to_owned()));
    }
    Ok(peers)
}

/// Top-level application settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub server: ServerSettings,
    pub storage: StorageSettings,
    pub scheduler: SchedulerSettings,
    pub model: ModelSettings,
    /// `None` runs a single node exactly as before.
    pub cluster: Option<ClusterSettings>,
}

#[derive(Deserialize)]
struct RawSettings {
    server: ServerSettings,
    #[serde(default)]
    storage: StorageSettings,
    #[serde(default)]
    scheduler: SchedulerSettings,
    #[serde(default)]
    model: ModelSettings,
    #[serde(default)]
    cluster: RawCluster,
}

/// Merge dotenv pairs and environment pairs into one configuration map.
///
/// Every key is lowercased before precedence is decided, so the map never holds
/// two spellings of the same config key (`config` lowercases keys itself and
/// would otherwise resolve such collisions in random `HashMap` order).
///
/// Precedence: inside the dotenv source the first occurrence of a key (in any
/// case) wins over later duplicates, and every environment entry overrides the
/// dotenv source whatever the case of either spelling. If the environment holds
/// the same key in two spellings (`SERVER.HOST` and `server.host`), the one that
/// comes later in the environment list wins; for `std::env::vars_os()` that
/// order is fixed for a given process environment. The first dotenv error is
/// returned before any pair is used, so a malformed file is never half-applied.
/// Environment entries whose name or value is not valid UTF-8 are skipped.
///
/// Only the exact keys in `CONFIG_KEYS` (after lowercasing) are kept, so
/// unrelated variables (`SERVER`, `PATH`, ...) and deeper or unknown `server.*`
/// names (`SERVER.HOST.X`, `SERVER.OTHER`) cannot collide with the settings
/// tables. The accepted names are the dotted `SERVER.HOST` / `SERVER.PORT` /
/// `STORAGE.PATH` (in any case); there is no double-underscore alias. The
/// returned keys are lowercase.
///
/// This function is pure: it touches neither the process environment nor the
/// filesystem.
pub fn merge_dotenv_env<I, E>(
    dotenv: Option<I>,
    env: E,
) -> Result<::config::Map<String, String>, ConfigError>
where
    I: IntoIterator<Item = dotenvy::Result<(String, String)>>,
    E: IntoIterator<Item = (OsString, OsString)>,
{
    let mut vars = ::config::Map::new();
    if let Some(items) = dotenv {
        for item in items {
            let (key, value) = item.map_err(dotenv_error)?;
            vars.entry(key.to_ascii_lowercase()).or_insert(value);
        }
    }
    for (key, value) in env {
        if let (Ok(key), Ok(value)) = (key.into_string(), value.into_string()) {
            vars.insert(key.to_ascii_lowercase(), value);
        }
    }
    vars.retain(|key, _| CONFIG_KEYS.contains(&key.as_str()));
    Ok(vars)
}

/// A dotenv failure as a configuration error whose text starts with `.env: `.
fn dotenv_error(e: dotenvy::Error) -> ConfigError {
    ConfigError::Message(format!(".env: {e}"))
}

impl Settings {
    /// Read the dotenv file (if present) and the process environment, modifying neither.
    pub fn load() -> Result<Self, ConfigError> {
        Self::from_sources(dotenvy::dotenv_iter(), std::env::vars_os())
    }

    /// Build settings from a dotenv lookup result and an environment list.
    ///
    /// A not-found dotenv result is ignored; any other dotenv error becomes a
    /// configuration error whose text starts with `.env: `.
    pub fn from_sources<R: std::io::Read, E>(
        dotenv: dotenvy::Result<dotenvy::Iter<R>>,
        env: E,
    ) -> Result<Self, ConfigError>
    where
        E: IntoIterator<Item = (OsString, OsString)>,
    {
        let file = match dotenv {
            Ok(iter) => Some(iter),
            Err(e) if e.not_found() => None,
            Err(e) => return Err(dotenv_error(e)),
        };
        Self::from_map(merge_dotenv_env(file, env)?)
    }

    /// Build settings from an explicit map of variable names to values.
    pub fn from_map(vars: ::config::Map<String, String>) -> Result<Self, ::config::ConfigError> {
        Self::from_env_source(Environment::default().source(Some(vars)))
    }

    fn from_env_source(env: Environment) -> Result<Self, ConfigError> {
        let raw: RawSettings = Config::builder()
            .add_source(env)
            .build()?
            .try_deserialize()?;
        if raw.model.path.is_some() != raw.model.tokenizer.is_some() {
            return Err(invalid(
                "MODEL.PATH and MODEL.TOKENIZER must be set together",
            ));
        }
        let cluster = raw.cluster.parse()?;
        if cluster.is_some() && raw.storage.path.is_none() {
            return Err(invalid("a cluster needs STORAGE.PATH"));
        }
        Ok(Self {
            server: raw.server,
            storage: raw.storage,
            scheduler: raw.scheduler,
            model: raw.model,
            cluster,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> ::config::Map<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(*k), OsString::from(*v)))
            .collect()
    }

    fn clustered(extra: &[(&str, &str)]) -> Result<Settings, ConfigError> {
        let mut pairs = vec![("SERVER.HOST", "h"), ("SERVER.PORT", "1")];
        pairs.extend_from_slice(extra);
        Settings::from_map(vars(&pairs))
    }

    #[test]
    fn three_peer_cluster_parses() {
        let s = clustered(&[
            ("STORAGE.PATH", "/d"),
            ("CLUSTER.NODE_ID", "2"),
            ("CLUSTER.PEERS", "1=a:1, 2=b:2,3=c:3"),
        ])
        .unwrap();
        let c = s.cluster.unwrap();
        assert_eq!(c.node_id, 2);
        assert_eq!(
            c.peers,
            vec![(1, "a:1".into()), (2, "b:2".into()), (3, "c:3".into())]
        );
    }

    #[test]
    fn the_snapshot_interval_defaults_to_ten_thousand_and_can_be_set_but_not_to_zero() {
        let with = |extra: &[(&str, &str)]| {
            let mut pairs = vec![
                ("STORAGE.PATH", "/d"),
                ("CLUSTER.NODE_ID", "1"),
                ("CLUSTER.PEERS", "1=a:1"),
            ];
            pairs.extend_from_slice(extra);
            clustered(&pairs).map(|s| s.cluster.unwrap().snapshot_every)
        };
        assert_eq!(with(&[]).unwrap(), 10_000);
        assert_eq!(with(&[("CLUSTER.SNAPSHOT_EVERY", "50")]).unwrap(), 50);
        let zero = with(&[("CLUSTER.SNAPSHOT_EVERY", "0")]).unwrap_err();
        assert!(zero.to_string().contains("SNAPSHOT_EVERY"), "{zero}");
    }

    #[test]
    fn the_shrink_delay_defaults_to_ten_minutes_and_can_be_set_but_not_to_zero() {
        let with = |extra: &[(&str, &str)]| {
            let mut pairs = vec![
                ("STORAGE.PATH", "/d"),
                ("CLUSTER.NODE_ID", "1"),
                ("CLUSTER.PEERS", "1=a:1"),
            ];
            pairs.extend_from_slice(extra);
            clustered(&pairs).map(|s| s.cluster.unwrap().shrink_after)
        };
        assert_eq!(with(&[]).unwrap(), std::time::Duration::from_secs(600));
        let set = with(&[("CLUSTER.SHRINK_AFTER", "5")]).unwrap();
        assert_eq!(set, std::time::Duration::from_secs(5));
        let zero = with(&[("CLUSTER.SHRINK_AFTER", "0")]).unwrap_err();
        assert!(zero.to_string().contains("SHRINK_AFTER"), "{zero}");
    }

    #[test]
    fn invalid_cluster_settings_are_refused_with_the_problem_named() {
        let cases: [(&[(&str, &str)], &str); 8] = [
            (
                &[("STORAGE.PATH", "/d"), ("CLUSTER.NODE_ID", "1")],
                "together",
            ),
            (
                &[("STORAGE.PATH", "/d"), ("CLUSTER.PEERS", "1=a:1")],
                "together",
            ),
            (
                &[
                    ("STORAGE.PATH", "/d"),
                    ("CLUSTER.NODE_ID", "1"),
                    ("CLUSTER.PEERS", "1=a:1,x"),
                ],
                "not id=host:port",
            ),
            (
                &[
                    ("STORAGE.PATH", "/d"),
                    ("CLUSTER.NODE_ID", "1"),
                    ("CLUSTER.PEERS", "1=a:1,1=b:2,2=c:3"),
                ],
                "repeats id 1",
            ),
            (
                &[
                    ("STORAGE.PATH", "/d"),
                    ("CLUSTER.NODE_ID", "1"),
                    ("CLUSTER.PEERS", ""),
                ],
                "not id=host:port",
            ),
            (
                &[
                    ("STORAGE.PATH", "/d"),
                    ("CLUSTER.NODE_ID", "9"),
                    ("CLUSTER.PEERS", "1=a:1"),
                ],
                "not in CLUSTER.PEERS",
            ),
            (
                &[
                    ("STORAGE.PATH", "/d"),
                    ("CLUSTER.NODE_ID", "1"),
                    ("CLUSTER.PEERS", "1=a:1,2=b:2"),
                ],
                "odd number",
            ),
            (
                &[("CLUSTER.NODE_ID", "1"), ("CLUSTER.PEERS", "1=a:1")],
                "STORAGE.PATH",
            ),
        ];
        for (extra, problem) in cases {
            let e = clustered(extra).unwrap_err().to_string();
            assert!(e.contains(problem), "{extra:?}: {e}");
        }
    }

    #[test]
    fn no_cluster_keys_means_no_cluster() {
        assert_eq!(clustered(&[]).unwrap().cluster, None);
    }

    #[test]
    fn dotted_keys_parse() {
        let s = Settings::from_map(vars(&[
            ("SERVER.HOST", "127.0.0.1"),
            ("SERVER.PORT", "5000"),
        ]))
        .expect("dotted keys map to server.host and server.port");
        assert_eq!(
            s,
            Settings {
                server: ServerSettings {
                    host: "127.0.0.1".into(),
                    port: 5000,
                },
                storage: StorageSettings::default(),
                scheduler: SchedulerSettings::default(),
                model: ModelSettings::default(),
                cluster: None,
            }
        );
        assert_eq!(s.scheduler.workers.get(), 2);
        assert_eq!(s.model.tokens_per_second.get(), 1000);
    }

    #[test]
    fn scheduler_and_model_are_read_in_any_case() {
        let merged = merge_dotenv_env(
            None::<Vec<dotenvy::Result<(String, String)>>>,
            env(&[
                ("SERVER.HOST", "h"),
                ("SERVER.PORT", "1"),
                ("Scheduler.Workers", "5"),
                ("model.TOKENS_per_second", "250"),
            ]),
        )
        .unwrap();
        let s = Settings::from_map(merged).unwrap();
        assert_eq!(s.scheduler.workers.get(), 5);
        assert_eq!(s.model.tokens_per_second.get(), 250);
    }

    #[test]
    fn zero_and_non_numeric_scheduler_and_model_values_are_errors() {
        for (key, value) in [
            ("SCHEDULER.WORKERS", "0"),
            ("SCHEDULER.WORKERS", "many"),
            ("MODEL.TOKENS_PER_SECOND", "0"),
            ("MODEL.TOKENS_PER_SECOND", "fast"),
        ] {
            let r = Settings::from_map(vars(&[
                ("SERVER.HOST", "h"),
                ("SERVER.PORT", "1"),
                (key, value),
            ]));
            assert!(r.is_err(), "{key}={value} accepted: {r:?}");
        }
    }

    #[test]
    fn model_files_and_threads_are_read_in_any_case_and_checked() {
        let base = [("SERVER.HOST", "h"), ("SERVER.PORT", "1")];
        let with = |extra: &[(&str, &str)]| Settings::from_map(vars(&[&base[..], extra].concat()));
        let s = with(&[
            ("Model.Path", "/m.gguf"),
            ("MODEL.tokenizer", "/t.json"),
            ("model.THREADS", "2"),
        ])
        .unwrap();
        assert_eq!(s.model.path, Some(PathBuf::from("/m.gguf")));
        assert_eq!(s.model.tokenizer, Some(PathBuf::from("/t.json")));
        assert_eq!(s.model.threads.get(), 2);
        assert_eq!(s.model.tokens_per_second.get(), 1000);
        assert_eq!(with(&[]).unwrap().model.threads.get(), 4);
        for extra in [
            &[("MODEL.PATH", "/m")][..],
            &[("MODEL.TOKENIZER", "/t")],
            &[("MODEL.THREADS", "0")],
            &[("MODEL.THREADS", "many")],
        ] {
            assert!(with(extra).is_err(), "{extra:?} accepted");
        }
        let e = with(&[("MODEL.PATH", "/m")]).unwrap_err().to_string();
        assert!(e.contains("MODEL.TOKENIZER"), "unexpected error: {e}");
    }

    #[test]
    fn storage_path_is_optional_and_read_in_any_case() {
        let merged = merge_dotenv_env(
            None::<Vec<dotenvy::Result<(String, String)>>>,
            env(&[
                ("SERVER.HOST", "h"),
                ("SERVER.PORT", "1"),
                ("Storage.Path", "/data/rs3.db"),
                ("STORAGE.OTHER", "ignored"),
            ]),
        )
        .unwrap();
        let s = Settings::from_map(merged).unwrap();
        assert_eq!(s.storage.path, Some(PathBuf::from("/data/rs3.db")));
    }

    #[test]
    fn missing_port_is_error() {
        let e = Settings::from_map(vars(&[("SERVER.HOST", "h")])).unwrap_err();
        assert!(e.to_string().contains("port"), "unexpected error: {e}");
    }

    #[test]
    fn missing_server_is_error() {
        let e = Settings::from_map(vars(&[])).unwrap_err();
        assert!(e.to_string().contains("server"), "unexpected error: {e}");
    }

    #[test]
    fn invalid_port_is_error() {
        let abc = Settings::from_map(vars(&[("SERVER.HOST", "h"), ("SERVER.PORT", "abc")]));
        assert!(abc.is_err(), "non-numeric port accepted: {abc:?}");
        let big = Settings::from_map(vars(&[("SERVER.HOST", "h"), ("SERVER.PORT", "70000")]));
        assert!(big.is_err(), "out-of-range port accepted: {big:?}");
    }

    #[test]
    fn underscore_names_are_not_mapped() {
        // The shell-friendly SERVER_HOST alias is deferred (OPS-02), not silently accepted.
        let r = Settings::from_map(vars(&[("SERVER_HOST", "h"), ("SERVER_PORT", "1")]));
        assert!(r.is_err(), "underscore names were mapped: {r:?}");
    }

    #[test]
    fn malformed_dotenv_is_error_with_env_prefix() {
        let content = "SERVER.HOST=a\nSERVER.PORT 5000\n";
        let e = Settings::from_sources(Ok(dotenvy::from_read_iter(content.as_bytes())), env(&[]))
            .unwrap_err();
        assert!(e.to_string().starts_with(".env: "), "unexpected error: {e}");
        let merged = merge_dotenv_env(Some(dotenvy::from_read_iter(content.as_bytes())), env(&[]));
        assert!(
            merged.is_err(),
            "malformed dotenv was half-loaded: {merged:?}"
        );
    }

    #[test]
    fn unreadable_dotenv_is_error() {
        let denied = dotenvy::Error::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        let e = Settings::from_sources::<&[u8], _>(
            Err(denied),
            env(&[("SERVER.HOST", "h"), ("SERVER.PORT", "1")]),
        )
        .unwrap_err();
        assert!(e.to_string().starts_with(".env: "), "unexpected error: {e}");
    }

    #[test]
    fn missing_dotenv_file_uses_env_only() {
        let missing = dotenvy::Error::Io(std::io::Error::from(std::io::ErrorKind::NotFound));
        let s = Settings::from_sources::<&[u8], _>(
            Err(missing),
            env(&[("SERVER.HOST", "h"), ("SERVER.PORT", "1")]),
        )
        .expect("a missing dotenv file is ignored");
        assert_eq!(s.server.host, "h");
        assert_eq!(s.server.port, 1);
    }

    #[test]
    fn dotenv_source_configures_settings() {
        let content = "SERVER.HOST=f\nSERVER.PORT=2\n";
        let s = Settings::from_sources(Ok(dotenvy::from_read_iter(content.as_bytes())), env(&[]))
            .expect("dotenv source supplies host and port");
        assert_eq!(s.server.host, "f");
        assert_eq!(s.server.port, 2);
    }

    fn not_found() -> dotenvy::Result<dotenvy::Iter<&'static [u8]>> {
        Err(dotenvy::Error::Io(std::io::Error::from(
            std::io::ErrorKind::NotFound,
        )))
    }

    #[test]
    fn merge_keeps_only_server_keys() {
        let content = "SERVER.HOST=a\nSERVER.PORT=1\n";
        let merged = merge_dotenv_env(
            Some(dotenvy::from_read_iter(content.as_bytes())),
            env(&[
                ("SERVER", "x"),
                ("OTHER", "1"),
                ("PATH", "/bin"),
                ("SERVER.HOST.X", "y"),
                ("SERVER.OTHER", "z"),
            ]),
        )
        .expect("valid dotenv content merges");
        let mut keys: Vec<&str> = merged.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["server.host", "server.port"]);
    }

    #[test]
    fn bare_server_var_does_not_break_settings() {
        // config applies keys in HashMap order, so repeat to catch an order-dependent collision.
        for _ in 0..32 {
            let s = Settings::from_sources(
                not_found(),
                env(&[("SERVER", "x"), ("SERVER.HOST", "h"), ("SERVER.PORT", "3")]),
            )
            .expect("a bare SERVER variable must not collide with the server table");
            assert_eq!(s.server.host, "h");
            assert_eq!(s.server.port, 3);
        }
    }

    #[test]
    fn nested_server_keys_do_not_break_settings() {
        // config applies keys in HashMap order, so repeat to catch an order-dependent collision.
        for _ in 0..32 {
            let s = Settings::from_sources(
                not_found(),
                env(&[
                    ("SERVER.HOST", "h"),
                    ("SERVER.HOST.X", "y"),
                    ("SERVER.PORT", "3"),
                    ("SERVER.PORT.ANYTHING", "z"),
                ]),
            )
            .expect("a deeper server.* variable must not collide with the server table");
            assert_eq!(s.server.host, "h");
            assert_eq!(s.server.port, 3);
        }
    }

    #[test]
    fn lowercase_server_keys_are_accepted() {
        let s = Settings::from_sources(
            not_found(),
            env(&[("server.host", "h"), ("server.port", "2")]),
        )
        .expect("lowercase dotted keys are accepted");
        assert_eq!(s.server.host, "h");
        assert_eq!(s.server.port, 2);
    }

    #[test]
    fn env_overrides_dotenv() {
        let merged = merge_dotenv_env(
            Some(dotenvy::from_read_iter("SERVER.HOST=file\n".as_bytes())),
            env(&[("SERVER.HOST", "env")]),
        )
        .expect("valid dotenv content merges");
        assert_eq!(merged.get("server.host").map(String::as_str), Some("env"));
    }

    #[test]
    fn env_overrides_dotenv_across_case() {
        // The env must win however either source spells the key. Before keys were
        // lowercased in the merge, both spellings survived and `config` picked one
        // in HashMap order, so check the merged map directly and repeat the full load.
        for (file_key, env_key) in [
            ("SERVER.HOST", "server.host"),
            ("server.host", "SERVER.HOST"),
        ] {
            let content = format!("{file_key}=file\nSERVER.PORT=1\n");
            let merged = merge_dotenv_env(
                Some(dotenvy::from_read_iter(content.as_bytes())),
                env(&[(env_key, "env")]),
            )
            .expect("valid dotenv content merges");
            assert_eq!(merged.len(), 2, "two spellings survived: {merged:?}");
            assert_eq!(merged.get("server.host").map(String::as_str), Some("env"));
            for _ in 0..64 {
                let s = Settings::from_sources(
                    Ok(dotenvy::from_read_iter(content.as_bytes())),
                    env(&[(env_key, "env")]),
                )
                .expect("valid sources configure");
                assert_eq!(s.server.host, "env");
            }
        }
    }

    #[test]
    fn dotenv_first_occurrence_wins() {
        let content = "SERVER.HOST=first\nSERVER.HOST=second\n";
        let merged = merge_dotenv_env(Some(dotenvy::from_read_iter(content.as_bytes())), env(&[]))
            .expect("valid dotenv content merges");
        assert_eq!(merged.get("server.host").map(String::as_str), Some("first"));
    }

    #[test]
    fn dotenv_first_occurrence_wins_across_case() {
        let content = "SERVER.HOST=first\nserver.host=second\nSERVER.PORT=1\n";
        let merged = merge_dotenv_env(Some(dotenvy::from_read_iter(content.as_bytes())), env(&[]))
            .expect("valid dotenv content merges");
        assert_eq!(merged.len(), 2, "two spellings survived: {merged:?}");
        assert_eq!(merged.get("server.host").map(String::as_str), Some("first"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_env_entries_are_skipped() {
        use std::os::unix::ffi::OsStringExt;
        let bad = || OsString::from_vec(vec![0xff]);
        let list = vec![
            (bad(), OsString::from("v")),
            (OsString::from("SERVER.HOST"), OsString::from("h")),
            (OsString::from("SERVER.PORT"), OsString::from("4")),
            (OsString::from("SERVER.HOST"), bad()),
        ];
        let merged = merge_dotenv_env(None::<dotenvy::Iter<&[u8]>>, list.clone())
            .expect("non-UTF-8 entries are skipped, not an error");
        assert_eq!(
            merged.get("server.host").map(String::as_str),
            Some("h"),
            "non-UTF-8 value replaced a valid one: {merged:?}"
        );
        assert_eq!(merged.len(), 2, "unexpected entries: {merged:?}");
        let s = Settings::from_sources(not_found(), list).expect("valid entries still configure");
        assert_eq!(s.server.host, "h");
        assert_eq!(s.server.port, 4);
    }

    #[test]
    fn bom_prefixed_dotenv_is_error() {
        let content = "\u{feff}SERVER.HOST=x\n";
        let e = Settings::from_sources(Ok(dotenvy::from_read_iter(content.as_bytes())), env(&[]))
            .unwrap_err();
        assert!(e.to_string().starts_with(".env: "), "unexpected error: {e}");
    }
}
