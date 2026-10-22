//! Blotter Sync: a headless Blotter that keeps every desktop's planner in
//! sync through one server.
//!
//! The planner logic (schema, operations, merging) lives in `blotter-core`,
//! shared with the desktop app. This crate adds the HTTP API, pairing,
//! storage for server-only state and the headless duties.

pub mod admin;
pub mod api;
pub mod auth;
pub mod changes;
pub mod config;
pub mod duties;
pub mod routes;
pub mod server;
pub mod store;

pub use routes::router;

/// Blotter Sync's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Port the server listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 7458;

#[cfg(test)]
mod sync_tests;
