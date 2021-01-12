use serde::Deserialize;
use dotenv::dotenv;
use config::{Config, ConfigError, Environment};

#[derive(Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Deserialize)]
pub  struct S3Config {
    pub server: ServerConfig,
}


impl S3Config {
    pub fn from_env() -> Result<Self, ConfigError> {

        dotenv().ok();
        let mut s3_config = Config::new();
        s3_config.merge(Environment::new()).unwrap();
        s3_config.try_into()
    }

}