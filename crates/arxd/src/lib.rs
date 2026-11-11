//! Authenticated foreground MCP server and a separate Unix execution library.
pub mod config;
pub mod exec;
pub mod http;
pub mod local;
mod mcp;
mod security;
pub mod service;
pub mod store;

pub mod development;
