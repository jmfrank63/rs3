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
    pub server: ServerConfig,
}

impl RS3Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenv().ok();
        let mut s3_config = Config::new();
        s3_config.merge(Environment::new()).unwrap();
        s3_config.try_into()
    }
}
