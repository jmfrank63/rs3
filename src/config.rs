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

    fn from_env_source(env: Environment) -> Result<Self, ConfigError> {
        Config::builder().add_source(env).build()?.try_deserialize()
    }
}
