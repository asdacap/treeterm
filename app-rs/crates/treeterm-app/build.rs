//! Lists pdf.js's data for the PDF viewer page, which `assets/pdf-viewer/build.sh` copies from
//! pdfjs-dist: its CMaps (`cmaps/*.bcmap`) and standard fonts (`standard_fonts/*.pfb`,
//! `*.ttf`). `$OUT_DIR/pdf_viewer_data.rs` holds `PDF_VIEWER_DATA`, each file's path in the page
//! and its bytes, for `browser_view.rs` to serve.

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let out = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let viewer = Path::new(&manifest).join("assets").join("pdf-viewer");
    let mut list = String::from("const PDF_VIEWER_DATA: &[(&str, &[u8])] = &[\n");
    for (dir, extensions) in [
        ("cmaps", &["bcmap"][..]),
        ("standard_fonts", &["pfb", "ttf"][..]),
    ] {
        let path = viewer.join(dir);
        println!("cargo::rerun-if-changed={}", path.display());
        let entries =
            std::fs::read_dir(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let mut names: Vec<String> = entries
            .map(|entry| {
                let name = entry
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
                    .file_name();
                name.into_string()
                    .unwrap_or_else(|name| panic!("{name:?} in {} is not UTF-8", path.display()))
            })
            .filter(|name| {
                name.rsplit_once('.')
                    .is_some_and(|(_, extension)| extensions.contains(&extension))
            })
            .collect();
        names.sort();
        assert!(
            !names.is_empty(),
            "no pdf.js data in {}: run assets/pdf-viewer/build.sh",
            path.display()
        );
        for name in names {
            let file = path.join(&name);
            let file = file
                .to_str()
                .unwrap_or_else(|| panic!("{} is not UTF-8", file.display()));
            writeln!(
                list,
                "    ({:?}, include_bytes!({file:?})),",
                format!("{dir}/{name}")
            )
            .expect("writing to a String cannot fail");
        }
    }
    list.push_str("];\n");
    let generated = Path::new(&out).join("pdf_viewer_data.rs");
    std::fs::write(&generated, list)
        .unwrap_or_else(|error| panic!("{}: {error}", generated.display()));
}
