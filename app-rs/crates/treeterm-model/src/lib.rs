//! TreeTerm's stores as GPUI entities.
//!
//! All application logic the UI shows lives here, so it can be tested headlessly with
//! `TestAppContext`; `treeterm-app` only renders these entities and dispatches to them.
//! I/O goes through `treeterm_core` and arrives via [`treeterm_core::rt`].

pub mod app_store;
pub mod session_store;
pub mod terminal;
