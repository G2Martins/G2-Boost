//! G2 Boost core library.
//!
//! Rules (see Docs/core/global-rules.md): every caller is hostile (UI, MCP,
//! config files, file names). Validation lives here, never in the UI. No
//! `cmd`/`powershell`/`reg`/`net`/`sc` spawning anywhere in this crate.

pub mod advisor;
pub mod apps;
pub mod boost;
pub mod cleanup;
pub mod context;
pub mod disk;
pub mod dns;
pub mod firmware;
pub mod hardware;
pub mod monitor;
pub mod network;
pub mod nvml;
pub mod power;
pub mod procs;
pub mod recommend;
pub mod registry;
pub mod restore;
pub mod routines;
pub mod safety;
pub mod secrets;
pub mod sensors;
pub mod settings;
pub mod startup;
pub mod steam;
pub mod system;
pub mod tasksched;
pub mod visualfx;
pub mod winsat;
pub mod wmi;

/// Crate version, surfaced in the UI "about" footer.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
