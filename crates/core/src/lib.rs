//! G2 Boost core library.
//!
//! Rules (see Docs/core/global-rules.md): every caller is hostile (UI, MCP,
//! config files, file names). Validation lives here, never in the UI. No
//! `cmd`/`powershell`/`reg`/`net` spawning anywhere in this crate.

pub mod disk;
pub mod network;
pub mod safety;
pub mod system;

/// Crate version, surfaced in the UI "about" footer.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
