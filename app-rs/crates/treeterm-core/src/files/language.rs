//! The editor language of a file, by extension (port of `utils/detectMonacoLanguage.ts`). The
//! ids are Monaco's, which is what Electron stores and what definition search switches on.

pub fn detect_monaco_language(file_path: &str) -> &'static str {
    let file_name = file_path.rsplit(['/', '\\']).next().unwrap_or(file_path);
    let Some((_, extension)) = file_name.rsplit_once('.') else {
        return "plaintext";
    };
    match extension.to_lowercase().as_str() {
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "json" => "json",
        "md" => "markdown",
        "css" => "css",
        "scss" => "scss",
        "less" => "less",
        "html" | "htm" | "vue" | "svelte" => "html",
        "xml" => "xml",
        "py" => "python",
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "cs" => "csharp",
        "c" | "h" => "c",
        "cpp" | "hpp" => "cpp",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "sh" | "bash" | "zsh" => "shell",
        "sql" => "sql",
        "graphql" | "gql" => "graphql",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "r" => "r",
        "lua" => "lua",
        "dockerfile" => "dockerfile",
        _ => "plaintext",
    }
}

#[cfg(test)]
mod tests {
    //! Port of `utils/detectMonacoLanguage.test.ts`.

    use super::*;

    mod detect_monaco_language {
        use super::*;

        #[test]
        fn detects_each_extension() {
            let cases = [
                ("src/example.ts", "typescript"),
                ("src/example.tsx", "typescript"),
                ("src/example.js", "javascript"),
                ("src/example.jsx", "javascript"),
                ("src/example.json", "json"),
                ("src/example.md", "markdown"),
                ("src/example.css", "css"),
                ("src/example.scss", "scss"),
                ("src/example.less", "less"),
                ("src/example.html", "html"),
                ("src/example.htm", "html"),
                ("src/example.vue", "html"),
                ("src/example.svelte", "html"),
                ("src/example.xml", "xml"),
                ("src/example.py", "python"),
                ("src/example.rs", "rust"),
                ("src/example.go", "go"),
                ("src/example.java", "java"),
                ("src/example.cs", "csharp"),
                ("src/example.c", "c"),
                ("src/example.h", "c"),
                ("src/example.cpp", "cpp"),
                ("src/example.hpp", "cpp"),
                ("src/example.yaml", "yaml"),
                ("src/example.yml", "yaml"),
                ("src/example.toml", "toml"),
                ("src/example.sh", "shell"),
                ("src/example.bash", "shell"),
                ("src/example.zsh", "shell"),
                ("src/example.sql", "sql"),
                ("src/example.graphql", "graphql"),
                ("src/example.gql", "graphql"),
                ("src/example.rb", "ruby"),
                ("src/example.php", "php"),
                ("src/example.swift", "swift"),
                ("src/example.kt", "kotlin"),
                ("src/example.kts", "kotlin"),
                ("src/example.scala", "scala"),
                ("src/example.r", "r"),
                ("src/example.lua", "lua"),
                ("src/example.dockerfile", "dockerfile"),
            ];
            for (file_path, expected) in cases {
                assert_eq!(detect_monaco_language(file_path), expected, "{file_path}");
            }
        }

        #[test]
        fn detects_c_sharp_case_insensitively_from_a_windows_path() {
            assert_eq!(detect_monaco_language("C:\\repo\\Example.CS"), "csharp");
        }

        #[test]
        fn falls_back_to_plaintext() {
            for file_path in ["README", "src/example.unknown", "dir.d/README"] {
                assert_eq!(
                    detect_monaco_language(file_path),
                    "plaintext",
                    "{file_path}"
                );
            }
        }
    }
}
