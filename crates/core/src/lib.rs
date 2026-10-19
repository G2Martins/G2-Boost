//! G2 Boost core library.
//!
//! Rules (see Docs/core/global-rules.md): every caller is hostile (UI, MCP,
//! config files, file names). Validation lives here, never in the UI. No
//! `cmd`/`powershell`/`reg`/`net`/`sc` spawning anywhere in this crate.

pub mod advisor;
pub mod apps;
pub mod boost;
pub mod boottime;
pub mod cleanup;
pub mod context;
pub mod disk;
pub mod dupes;
pub mod dns;
pub mod firmware;
pub mod gamemode;
pub mod games;
pub mod hardware;
pub mod icons;
pub mod winevents;
pub mod claude_connect;
pub mod console;
pub mod consent;
pub mod devices;
pub mod luli;
pub mod winget;
pub mod monitor;
pub mod network;
pub mod netconns;
pub mod nvml;
pub mod power;
pub mod prio;
pub mod procs;
pub mod recommend;
pub mod registry;
pub mod restore;
pub mod routines;
pub mod safety;
pub mod secrets;
pub mod sensors;
pub mod settings;
pub mod smart;
pub mod startup;
pub mod steam;
pub mod system;
pub mod tasksched;
pub mod userassist;
pub mod visualfx;
pub mod winsat;
pub mod wmi;
pub mod wlan;

/// Crate version, surfaced in the UI "about" footer.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
