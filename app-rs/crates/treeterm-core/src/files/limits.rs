//! How many bytes a viewer pulls down for one file (port of `renderer/lib/fileLimits.ts`).
//!
//! A transfer budget, enforced against the read's header so an oversize file is refused
//! before its content crosses the wire. Unrelated to the daemon's poll-backed watch cap.

use super::content_kind::{is_model_path, is_pdf_path};

pub use crate::fs::MAX_READ_FILE_BYTES;

/// 3D models (meshes for printing, scanned point clouds) routinely run to tens of megabytes.
pub const MAX_READ_MODEL_BYTES: u64 = 64 * 1024 * 1024;

/// Scanned documents and exported slide decks are routinely several megabytes, often tens.
pub const MAX_READ_PDF_BYTES: u64 = 64 * 1024 * 1024;

/// The transfer budget for opening `file_path` in a viewer.
pub fn max_read_bytes_for(file_path: &str) -> u64 {
    if is_model_path(file_path) {
        MAX_READ_MODEL_BYTES
    } else if is_pdf_path(file_path) {
        MAX_READ_PDF_BYTES
    } else {
        MAX_READ_FILE_BYTES
    }
}

#[cfg(test)]
mod tests {
    //! Port of `lib/fileLimits.test.ts`.

    use super::*;

    mod max_read_bytes_for {
        use super::*;

        #[test]
        fn gives_3d_models_and_pdfs_their_larger_budgets_and_every_other_file_the_default() {
            assert_eq!(
                max_read_bytes_for("/repo/parts/bracket.stl"),
                MAX_READ_MODEL_BYTES
            );
            assert_eq!(
                max_read_bytes_for("/repo/docs/Manual.PDF"),
                MAX_READ_PDF_BYTES
            );
            assert_eq!(max_read_bytes_for("/repo/src/main.ts"), MAX_READ_FILE_BYTES);
            assert_eq!(max_read_bytes_for("/repo/logo.png"), MAX_READ_FILE_BYTES);
        }
    }
}
