//! Shared local Docker Engine adapter. See ADR-019 and docs/docker.md.

mod contexts;
mod engine;
mod logs;
mod service;

pub use service::NativeDocker;
mod exec;
pub use exec::EngineTerminal;
#[cfg(all(test, windows))]
mod exec_tests;
