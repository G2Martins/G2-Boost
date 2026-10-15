//! G2 Boost core library.
//!
//! Rules (see Docs/core/global-rules.md): every caller is hostile (UI, MCP,
//! config files, file names). Validation lives here, never in the UI. No
//! `cmd`/`powershell`/`reg`/`net`/`sc` spawning anywhere in this crate.

pub mod boost;
pub mod cleanup;
pub mod disk;
pub mod hardware;
pub mod monitor;
pub mod network;
pub mod recommend;
pub mod registry;
pub mod restore;
pub mod routines;
pub mod safety;
pub mod settings;
pub mod system;
pub mod tasksched;
pub mod visualfx;

/// Crate version, surfaced in the UI "about" footer.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
