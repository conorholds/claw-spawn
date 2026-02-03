//! Claw Spawn
//!
//! DigitalOcean VPS provisioning + OpenClaw bot orchestration.
//!
//! ## Standalone
//!
//! Run the binary:
//! ```bash
//! claw-spawn-server
//! ```
//!
//! ## Embedded (Axum)
//!
//! When the `server` feature is enabled, this crate can be embedded into a larger Axum app:
//! ```rust,ignore
//! use axum::Router;
//! use claw_spawn::infrastructure::AppConfig;
//! use claw_spawn::server::{build_state_with_pool, router};
//! use sqlx::PgPool;
//!
//! let cfg = AppConfig::from_env()?;
//! let pool = PgPool::connect(&cfg.database_url).await?;
//! let state = build_state_with_pool(cfg, pool, true).await?;
//! let app = Router::new().nest("/spawn", router(state));
//! ```

pub mod application;
pub mod domain;
pub mod infrastructure;

// Standalone + embedded HTTP server support (Axum).
// Enabled behind the `server` feature so the core library can be used without Axum.
#[cfg(feature = "server")]
pub mod server;

pub use application::*;
pub use domain::*;
pub use infrastructure::*;

#[cfg(feature = "server")]
pub use server::*;
