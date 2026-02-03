use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub digitalocean_token: String,
    pub encryption_key: String,
    pub server_host: String,
    pub server_port: u16,
    pub openclaw_image: String,
    pub openclaw_bootstrap_url: String,
    pub control_plane_url: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(File::with_name("config/default").required(false))
            .add_source(File::with_name("config/local").required(false))
            .add_source(Environment::with_prefix("CEDROS").separator("_"))
            .set_default("server_host", "0.0.0.0")?
            .set_default("server_port", 8080)?
            .set_default("openclaw_image", "ubuntu-22-04-x64")?
            .set_default(
                "openclaw_bootstrap_url",
                "https://install.openclaw.dev/bootstrap.sh",
            )?
            .set_default("control_plane_url", "https://api.cedros.io")?
            .build()?;

        config.try_deserialize()
    }
}
