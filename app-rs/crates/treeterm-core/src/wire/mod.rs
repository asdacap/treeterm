//! On-disk and on-wire formats shared with the Electron app. Field names and optionality
//! mirror the Zod schemas exactly so both apps can work on the same session.

pub mod workspace_file;
pub mod stable_json;
