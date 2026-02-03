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
