//! Server configuration.
//!
//! Loads a dotenv file if one is present (it never overrides variables that are
//! already set) and then reads `SERVER.HOST` and `SERVER.PORT` from the process
//! environment. There are no defaults: missing or invalid values are an error.
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

impl Settings {
    /// Load the dotenv file (if present), then read the process environment.
    pub fn load() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();
        Self::from_env_source(Environment::default())
    }

    /// Build settings from an explicit map instead of the process environment (for tests: edition 2024 makes env mutation unsafe, and unsafe code is forbidden here).
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
}
