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
    pub control_plane_url: String,

    // Workspace/customization (janebot-cli)
    pub customizer_repo_url: String,
    pub customizer_ref: String,
    pub customizer_workspace_dir: String,
    pub customizer_agent_name: String,
    pub customizer_owner_name: String,
    pub customizer_skip_qmd: bool,
    pub customizer_skip_cron: bool,
    pub customizer_skip_git: bool,
    pub customizer_skip_heartbeat: bool,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(File::with_name("config/default").required(false))
            .add_source(File::with_name("config/local").required(false))
            .add_source(Environment::with_prefix("CLAW").separator("_"))
            .set_default("server_host", "0.0.0.0")?
            .set_default("server_port", 8080)?
            .set_default("openclaw_image", "ubuntu-22-04-x64")?
            .set_default("control_plane_url", "https://api.example.com")?
            // janebot-cli customization defaults (pinned for reproducibility)
            .set_default(
                "customizer_repo_url",
                "https://github.com/janebot2026/janebot-cli.git",
            )?
            .set_default("customizer_ref", "4b170b4aa31f79bda84f7383b3992ca8681d06d3")?
            .set_default("customizer_workspace_dir", "/opt/openclaw/workspace")?
            .set_default("customizer_agent_name", "Jane")?
            .set_default("customizer_owner_name", "Cedros")?
            .set_default("customizer_skip_qmd", true)?
            .set_default("customizer_skip_cron", true)?
            .set_default("customizer_skip_git", true)?
            .set_default("customizer_skip_heartbeat", true)?
            .build()?;

        config.try_deserialize()
    }
}
