//! TreeTerm's stores as GPUI entities.
//!
//! All application logic the UI shows lives here, so it can be tested headlessly with
//! `TestAppContext`; `treeterm-app` only renders these entities and dispatches to them.
//! I/O goes through `treeterm_core` and arrives via [`treeterm_core::rt`].

pub mod active_processes;
pub mod analyzer;
pub mod app_store;
pub mod apps;
pub mod file_browser;
pub mod git_controller;
pub mod keybinding_store;
pub mod review;
pub mod session_info;
mod review_metadata;
pub mod run_actions;
pub mod session_store;
pub mod settings_store;
pub mod terminal;
pub mod text_editor;
pub mod web_app;
pub mod workspace_store;
