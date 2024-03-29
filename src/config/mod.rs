//! Contains the configuration for the server
//!
//! Either reads from the .env file if it exists
//! or reads from the environment
use config::{ConfigError, Environment};
use dotenv::dotenv;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Deserialize)]
pub struct Config {
    pub server: ServerConfig,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenv().ok();
        let mut cfg = config::Config::new();
        cfg.merge(Environment::new())?;
        cfg.try_into()
    }
}
