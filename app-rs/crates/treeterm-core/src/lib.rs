//! TreeTerm's application logic, free of any UI framework.
//!
//! Everything here is plain Rust driven by tokio so it can be unit-tested without a window.
//! The GPUI layers (`treeterm-model`, `treeterm-app`) consume it through [`rt`], which hands
//! tokio-driven work to any executor as plain futures and streams.

pub mod activity;
pub mod analyzer;
pub mod attention;
pub mod classification;
pub mod concurrency;
pub mod daemon;
pub mod exec;
pub mod fs;
pub mod git;
pub mod human_id;
pub mod keybinding;
pub mod layout;
pub mod llm;
pub mod local_daemon;
pub mod paths;
pub mod pty;
pub mod rt;
pub mod settings;
pub mod transport;
pub mod tree;
pub mod wire;
pub mod workspace_io;
