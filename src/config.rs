//! Server configuration.
//!
//! Settings come from the dotenv file (looked up in the current directory, then
//! its ancestors) overlaid by the process environment, which wins. The process
//! environment is only read, never modified. A missing dotenv file is ignored;
//! any other dotenv problem (a line that does not parse, an unreadable file) is
//! a configuration error. `SERVER.HOST` and `SERVER.PORT` are required: there
//! are no defaults, and missing or invalid values are an error.
use std::ffi::OsString;

use config::{Config, ConfigError, Environment};
use serde::Deserialize;

/// Address the HTTP server binds to.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
}

/// Top-level application settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Settings {
    pub server: ServerSettings,
}

/// Merge dotenv pairs and environment pairs into one configuration map.
///
/// Precedence: inside the dotenv source the first occurrence of a key wins over
/// later duplicates, and every environment entry overrides the dotenv source.
/// The first dotenv error is returned before any pair is used, so a malformed
/// file is never half-applied. Environment entries whose name or value is not
/// valid UTF-8 are skipped.
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
            vars.entry(key).or_insert(value);
        }
    }
    for (key, value) in env {
        if let (Ok(key), Ok(value)) = (key.into_string(), value.into_string()) {
            vars.insert(key, value);
        }
    }
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
        Config::builder().add_source(env).build()?.try_deserialize()
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
            }
        );
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
}
