//! What kind of content a file holds, by extension (port of `shared/fileContentKind.ts`).
//! Images, 3D models and PDFs are drawn from their raw bytes, which travel base64-encoded.

/// `FileContents.language` of a file read as an image.
pub const IMAGE_LANGUAGE: &str = "image";
/// `FileContents.language` of a file read as a 3D model.
pub const MODEL_LANGUAGE: &str = "model";
/// `FileContents.language` of a file read as a PDF.
pub const PDF_LANGUAGE: &str = "pdf";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelFormat {
    Stl,
    Obj,
    Ply,
    ThreeMf,
    Glb,
    Gltf,
}

impl ModelFormat {
    /// The extension, as the TS enum's value.
    pub fn as_str(self) -> &'static str {
        match self {
            ModelFormat::Stl => "stl",
            ModelFormat::Obj => "obj",
            ModelFormat::Ply => "ply",
            ModelFormat::ThreeMf => "3mf",
            ModelFormat::Glb => "glb",
            ModelFormat::Gltf => "gltf",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Not a 3D model file: {path}")]
pub struct NotAModel {
    pub path: String,
}

/// The text after the last `.`, lowercased; the whole path when it has no `.`.
fn extension_of(file_path: &str) -> String {
    file_path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

fn image_mime(extension: &str) -> Option<&'static str> {
    match extension {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        "avif" => Some("image/avif"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

fn model(extension: &str) -> Option<ModelFormat> {
    match extension {
        "stl" => Some(ModelFormat::Stl),
        "obj" => Some(ModelFormat::Obj),
        "ply" => Some(ModelFormat::Ply),
        "3mf" => Some(ModelFormat::ThreeMf),
        "glb" => Some(ModelFormat::Glb),
        "gltf" => Some(ModelFormat::Gltf),
        _ => None,
    }
}

pub fn is_image_path(file_path: &str) -> bool {
    image_mime(&extension_of(file_path)).is_some()
}

/// Only meaningful for a path that passed [`is_image_path`].
pub fn image_mime_type(file_path: &str) -> &'static str {
    image_mime(&extension_of(file_path)).unwrap_or("application/octet-stream")
}

pub fn is_model_path(file_path: &str) -> bool {
    model(&extension_of(file_path)).is_some()
}

pub fn model_format(file_path: &str) -> Result<ModelFormat, NotAModel> {
    model(&extension_of(file_path)).ok_or_else(|| NotAModel {
        path: file_path.to_owned(),
    })
}

pub fn is_pdf_path(file_path: &str) -> bool {
    extension_of(file_path) == "pdf"
}

/// Files drawn from their raw bytes: a read sends these base64-encoded, since decoding them
/// as UTF-8 text would mangle the bytes.
pub fn is_raw_bytes_path(file_path: &str) -> bool {
    is_image_path(file_path) || is_model_path(file_path) || is_pdf_path(file_path)
}

/// The `FileContents.language` a read reports: raw-bytes files are tagged by kind.
pub fn read_language<'a>(file_path: &str, daemon_language: &'a str) -> &'a str {
    if is_image_path(file_path) {
        IMAGE_LANGUAGE
    } else if is_model_path(file_path) {
        MODEL_LANGUAGE
    } else if is_pdf_path(file_path) {
        PDF_LANGUAGE
    } else {
        daemon_language
    }
}

/// Git's own heuristic: a NUL within the first 8000 characters marks the content binary.
const BINARY_SNIFF_LENGTH: usize = 8000;

pub fn is_binary_content(content: &str) -> bool {
    content.chars().take(BINARY_SNIFF_LENGTH).any(|c| c == '\0')
}

#[cfg(test)]
mod tests {
    //! Port of `shared/fileContentKind.test.ts`.

    use super::*;

    mod file_content_kind {
        use super::*;

        #[test]
        fn recognises_image_extensions_case_insensitively() {
            assert!(is_image_path("a/logo.PNG"));
            assert!(is_image_path("a/icon.svg"));
            assert!(!is_image_path("a/app.ts"));
            assert!(!is_image_path("Makefile"));
        }

        #[test]
        fn maps_image_extensions_to_mime_types() {
            assert_eq!(image_mime_type("photo.jpg"), "image/jpeg");
            assert_eq!(image_mime_type("icon.svg"), "image/svg+xml");
            assert_eq!(image_mime_type("data.bin"), "application/octet-stream");
        }

        #[test]
        fn recognises_3d_model_extensions_case_insensitively() {
            assert!(is_model_path("parts/bracket.STL"));
            assert!(is_model_path("scene.glb"));
            assert!(is_model_path("print.3mf"));
            assert!(!is_model_path("logo.png"));
            assert!(!is_model_path("README"));
        }

        #[test]
        fn maps_a_model_path_to_its_format_and_refuses_any_other_path() {
            assert_eq!(model_format("a/b/Bracket.Stl"), Ok(ModelFormat::Stl));
            assert_eq!(model_format("mesh.obj"), Ok(ModelFormat::Obj));
            assert_eq!(model_format("scan.ply"), Ok(ModelFormat::Ply));
            assert_eq!(model_format("print.3mf"), Ok(ModelFormat::ThreeMf));
            assert_eq!(model_format("scene.gltf"), Ok(ModelFormat::Gltf));
            assert_eq!(
                model_format("notes.txt").unwrap_err().to_string(),
                "Not a 3D model file: notes.txt"
            );
            assert_eq!(ModelFormat::ThreeMf.as_str(), "3mf");
        }

        #[test]
        fn recognises_pdfs_case_insensitively() {
            assert!(is_pdf_path("docs/Manual.PDF"));
            assert!(!is_pdf_path("docs/manual.pdf.txt"));
        }

        #[test]
        fn reads_images_models_and_pdfs_as_raw_bytes_everything_else_as_text() {
            assert!(is_raw_bytes_path("logo.png"));
            assert!(is_raw_bytes_path("part.stl"));
            assert!(is_raw_bytes_path("manual.pdf"));
            assert!(!is_raw_bytes_path("main.ts"));
        }

        #[test]
        fn tags_raw_bytes_reads_by_kind_and_keeps_the_daemon_language_otherwise() {
            assert_eq!(read_language("logo.png", "plaintext"), IMAGE_LANGUAGE);
            assert_eq!(read_language("part.stl", "plaintext"), MODEL_LANGUAGE);
            assert_eq!(read_language("manual.pdf", "plaintext"), PDF_LANGUAGE);
            assert_eq!(read_language("main.ts", "typescript"), "typescript");
        }

        #[test]
        fn detects_binary_content_by_a_leading_nul_byte() {
            assert!(!is_binary_content("hello\nworld"));
            assert!(!is_binary_content(""));
            assert!(is_binary_content("\u{89}PNG\r\n\u{1a}\n\0\0"));
            assert!(!is_binary_content(&format!("{}\0", "x".repeat(8000))));
        }
    }
}
