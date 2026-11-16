//! TreeTerm's stores as GPUI entities.
//!
//! All application logic the UI shows lives here, so it can be tested headlessly with
//! `TestAppContext`; `treeterm-app` only renders these entities and dispatches to them.
//! I/O goes through `treeterm_core` and arrives via [`treeterm_core::rt`].

pub mod active_processes;
pub mod analyzer;
pub mod app_store;
pub mod auto_open;
pub mod bracket_pairs;
pub mod code_intel;
pub mod browser_page;
pub mod apps;
pub mod child_dialog;
pub mod definition;
pub mod dir_browser;
pub mod file_browser;
pub mod forward_monitor;
pub mod git_controller;
pub mod harness_prompts;
pub mod indentation;
pub mod keybinding_store;
pub mod navigation;
pub mod pty_writer;
pub mod review;
pub mod review_find;
pub mod review_patch;
pub mod review_tree;
pub mod search_panel;
pub mod session_info;
pub mod session_names;
mod review_metadata;
pub mod run_actions;
pub mod session_store;
pub mod session_tty_list;
pub mod settings_store;
pub mod ssh_connection_form;
pub mod system_monitor;
pub mod terminal;
pub mod text_editor;
pub mod tool_tabs;
pub mod traffic;
pub mod tree_view_state;
pub mod web_app;
pub mod words;
pub mod workspace_store;
pub mod zoom;
