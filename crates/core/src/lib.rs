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
pub mod netetw;
pub mod nvml;
pub mod power;
pub mod prio;
pub mod procs;
pub mod qos;
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
pub mod repair;
pub mod volcache;
pub mod cpusets;
pub mod nvapi;
pub mod adlx;
pub mod gpu;
pub mod exposure;
pub mod updates;
pub mod volumes;
pub mod netrepair;
// r9:games
pub mod covers;

// r9:tweaks
pub mod debloat;
pub mod winfeature;

// r9:maint

// r9:hw
pub mod checkup;
pub mod hwcsv;
pub mod hwsensors;

// r9:install
pub mod catalog;
pub mod install;

// r9:tools
pub mod pktloss;

// r9:overlay
pub mod fps;
pub mod overlay;

// r9:profiles
pub mod peripherals;
pub mod profiles;


// r9:setup


/// Crate version, surfaced in the UI "about" footer.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
