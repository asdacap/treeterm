//! Terminal emulation for TreeTerm, free of any UI framework.
//!
//! Built on `alacritty_terminal`. The emulator is fed from the daemon's PTY stream (the
//! daemon owns the PTY and its size); this crate parses output, answers queries under the
//! focus/replay gate, encodes keyboard and mouse input, and answers questions about the
//! buffer for the UI, activity detection and tests.

pub mod core;
pub mod fit;
pub mod idle;
pub mod keys;
pub mod links;
pub mod mouse;
pub mod move_to_cell;
pub mod palette;
pub mod raw_chars;
pub mod view;

pub use alacritty_terminal;
