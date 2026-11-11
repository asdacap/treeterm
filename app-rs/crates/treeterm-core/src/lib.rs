//! TreeTerm's application logic, free of any UI framework.
//!
//! Everything here is plain Rust driven by tokio so it can be unit-tested without a window.
//! The GPUI layers (`treeterm-model`, `treeterm-app`) consume it through [`rt`], which hands
//! tokio-driven work to any executor as plain futures and streams.

pub mod activity;
pub mod analyzer;
pub mod attention;
pub mod classification;
pub mod clock;
pub mod concurrency;
pub mod connection;
pub mod daemon;
pub mod exec;
pub mod files;
pub mod fs;
pub mod fs_ext;
pub mod git;
pub mod git_status;
pub mod github;
pub mod http;
pub mod human_id;
pub mod keybinding;
pub mod layout;
pub mod llm;
pub mod local_daemon;
pub mod paths;
pub mod pty;
pub mod review;
pub mod rt;
pub mod run_actions;
pub mod search;
pub mod settings;
pub mod system_monitor;
pub mod traffic;
pub mod transport;
pub mod tree;
pub mod wire;
pub mod workspace_io;
pub mod worktree_hierarchy;
pub mod worktree_registry;

#[cfg(test)]
mod fakes;
