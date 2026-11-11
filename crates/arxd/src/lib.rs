//! Inventory-only foreground server and a separate, unexposed execution library.
pub mod config;
pub mod exec;
pub mod http;
pub mod local;
mod mcp;
mod security;
pub mod store;
