//! User-scoped Linux diagnostics. Collection, analysis and presentation are separate.
pub mod cgroup;
pub mod checks;
pub mod cli;
pub mod config;
pub mod engine;
pub mod model;
pub mod oom;
pub mod procfs;
pub mod render;
pub mod theme;
pub mod tui;
pub mod util;

pub type Result<T> = std::result::Result<T, String>;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
extern crate self as ures;
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod test_support;
