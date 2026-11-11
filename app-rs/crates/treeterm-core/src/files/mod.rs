//! File classification and file-browser helpers: content kind, read budgets, editor
//! language, favourites.

pub mod content_kind;
pub mod favourites;
pub mod language;
pub mod limits;

pub use content_kind::{
    IMAGE_LANGUAGE, MODEL_LANGUAGE, ModelFormat, NotAModel, PDF_LANGUAGE, image_mime_type,
    is_binary_content, is_image_path, is_model_path, is_pdf_path, is_raw_bytes_path, model_format,
    read_language,
};
pub use favourites::{
    FavouriteFile, FavouritesError, favourite_file_name, normalize_favourite_path,
    normalize_file_entry_relative_path, resolve_favourite_files,
};
pub use language::detect_monaco_language;
pub use limits::{
    MAX_READ_FILE_BYTES, MAX_READ_MODEL_BYTES, MAX_READ_PDF_BYTES, max_read_bytes_for,
};
