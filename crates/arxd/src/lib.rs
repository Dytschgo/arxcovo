//! Authenticated foreground MCP server and a separate Unix execution library.
mod client_config;
pub mod config;
pub mod dashboard;
pub mod exec;
pub mod http;
pub mod local;
mod mcp;
mod security;
pub mod service;
pub mod store;
pub mod vault;
pub mod vm;

pub mod cluster;
pub mod development;
#[cfg(any(unix, test))]
pub mod unix_shim;
