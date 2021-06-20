use config::{Config, ConfigError, Environment};
use dotenv::dotenv;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Deserialize)]
pub struct RS3Config {
    pub rs3_server: ServerConfig,
}

impl RS3Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenv().ok();
        let mut rs3_config = Config::new();
        rs3_config.merge(Environment::new())?;
        rs3_config.try_into()
    }
}
