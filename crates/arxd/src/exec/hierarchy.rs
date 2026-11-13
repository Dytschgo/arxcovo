//! Inactive ADR 0020 hierarchy contract. E1 is pure data parsing only.
#![cfg_attr(not(test), allow(dead_code))]

pub(crate) mod parse;
#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use parse::locate;

#[cfg(test)]
mod fake;
pub(crate) mod setup;
