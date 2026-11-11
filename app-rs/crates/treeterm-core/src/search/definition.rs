//! Go to definition by grepping for a definition pattern in the workspace (port of
//! `utils/definitionSearch.ts`). Languages are Monaco language ids.
//!
//! Unlike the TS version, a failure to run grep is an error rather than "no definitions".
//! grep's exit code is not: it exits 1 for no matches and 2 when some file was unreadable,
//! with the matches it found still printed.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::exec::{
    DEFAULT_EXEC_TIMEOUT, ExecApi, ExecError, ExecRequest, ExecTimeout, collect_output,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionLocation {
    /// Relative to the workspace.
    pub file_path: String,
    pub line_number: u64,
}

/// Directories never searched.
const EXCLUDED_DIRS: [&str; 5] = ["node_modules", ".git", "dist", "build", "target"];

/// `symbol` with regex metacharacters escaped (for `grep -E`).
fn escape_regex(symbol: &str) -> String {
    let mut escaped = String::with_capacity(symbol.len());
    for c in symbol.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// The extended regex matching a definition of `symbol` in `language`.
pub fn definition_pattern(symbol: &str, language: &str) -> String {
    let escaped = escape_regex(symbol);
    match language {
        "typescript" | "javascript" => {
            format!(r"(function|const|let|var|class|interface|type|enum)\s+{escaped}\b")
        }
        "python" => format!(r"(def|class)\s+{escaped}\b"),
        "rust" => format!(r"(fn|struct|enum|trait|type|const|static|mod|impl)\s+{escaped}\b"),
        "go" => format!(r"(func|type|var|const)\s+{escaped}\b"),
        "java" | "kotlin" => {
            format!(r"(class|interface|enum|void|public|private|protected)\s+.*{escaped}\b")
        }
        "ruby" => format!(r"(def|class|module)\s+{escaped}\b"),
        "php" => format!(r"(function|class|interface|trait)\s+{escaped}\b"),
        _ => format!(r"(function|class|def|fn|type|interface|struct|enum)\s+{escaped}\b"),
    }
}

/// `grep --include` globs for `language`; none searches every file.
pub fn file_globs(language: &str) -> &'static [&'static str] {
    match language {
        "typescript" => &["*.ts", "*.tsx"],
        "javascript" => &["*.js", "*.jsx", "*.mjs", "*.cjs"],
        "python" => &["*.py"],
        "rust" => &["*.rs"],
        "go" => &["*.go"],
        "java" => &["*.java"],
        "kotlin" => &["*.kt", "*.kts"],
        "ruby" => &["*.rb"],
        "php" => &["*.php"],
        "css" | "scss" | "less" => &["*.css", "*.scss", "*.less"],
        "html" => &["*.html", "*.htm"],
        _ => &[],
    }
}

fn grep_args(symbol: &str, language: &str) -> Vec<String> {
    let mut args = vec!["-rnE".to_owned(), definition_pattern(symbol, language)];
    args.extend(
        file_globs(language)
            .iter()
            .map(|glob| format!("--include={glob}")),
    );
    args.extend(
        EXCLUDED_DIRS
            .iter()
            .map(|dir| format!("--exclude-dir={dir}")),
    );
    args.push(".".to_owned());
    args
}

static GREP_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+?):(\d+):").expect("valid regex"));

/// `grep -rn` output (`file:line:content`) as locations; other lines are skipped.
fn parse_grep_output(stdout: &str) -> Vec<DefinitionLocation> {
    stdout
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let captures = GREP_LINE.captures(line)?;
            let raw_path = &captures[1];
            Some(DefinitionLocation {
                file_path: raw_path.strip_prefix("./").unwrap_or(raw_path).to_owned(),
                line_number: captures[2].parse().ok()?,
            })
        })
        .collect()
}

/// Where `symbol` may be defined under `workspace_path`.
pub async fn search_definition(
    exec: &dyn ExecApi,
    workspace_path: &str,
    symbol: &str,
    language: &str,
) -> Result<Vec<DefinitionLocation>, ExecError> {
    let output = collect_output(exec.start(ExecRequest {
        cwd: workspace_path.to_owned(),
        command: "grep".to_owned(),
        args: grep_args(symbol, language),
        env: HashMap::new(),
        timeout: ExecTimeout::After(DEFAULT_EXEC_TIMEOUT),
    }))
    .await?;
    Ok(parse_grep_output(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(test)]
mod tests {
    //! Port of `utils/definitionSearch.test.ts`, case by case.

    use std::sync::Arc;

    use super::*;
    use crate::exec::ExecEvent;
    use crate::fakes::{FakeExec, FakeRun, block_on, exits, start_fails};

    /// Run a search against one scripted exec; returns the result and the grep args.
    fn search(
        run: FakeRun,
        symbol: &str,
        language: &str,
    ) -> (Result<Vec<DefinitionLocation>, ExecError>, Arc<FakeExec>) {
        let exec = FakeExec::new(vec![run]);
        let result = block_on(search_definition(exec.as_ref(), "/ws", symbol, language));
        (result, exec)
    }

    fn args(language: &str) -> Vec<String> {
        let (result, exec) = search(exits("", 0), "sym", language);
        result.unwrap();
        exec.call(1).args
    }

    fn location(file_path: &str, line_number: u64) -> DefinitionLocation {
        DefinitionLocation {
            file_path: file_path.to_owned(),
            line_number,
        }
    }

    mod search_definition_covers_all_language_switch_branches {
        use super::*;

        #[test]
        fn language_produces_correct_pattern() {
            let cases = [
                (
                    "typescript",
                    r"(function|const|let|var|class|interface|type|enum)\s+mySymbol\b",
                ),
                (
                    "javascript",
                    r"(function|const|let|var|class|interface|type|enum)\s+mySymbol\b",
                ),
                ("python", r"(def|class)\s+mySymbol\b"),
                (
                    "rust",
                    r"(fn|struct|enum|trait|type|const|static|mod|impl)\s+mySymbol\b",
                ),
                ("go", r"(func|type|var|const)\s+mySymbol\b"),
                (
                    "java",
                    r"(class|interface|enum|void|public|private|protected)\s+.*mySymbol\b",
                ),
                (
                    "kotlin",
                    r"(class|interface|enum|void|public|private|protected)\s+.*mySymbol\b",
                ),
                ("ruby", r"(def|class|module)\s+mySymbol\b"),
                ("php", r"(function|class|interface|trait)\s+mySymbol\b"),
                (
                    "unknown_lang",
                    r"(function|class|def|fn|type|interface|struct|enum)\s+mySymbol\b",
                ),
            ];
            for (language, expected) in cases {
                let exec = FakeExec::new(vec![exits("", 0)]);
                block_on(search_definition(
                    exec.as_ref(),
                    "/workspace",
                    "mySymbol",
                    language,
                ))
                .unwrap();
                let call = exec.call(1);
                assert_eq!(
                    (call.cwd.as_str(), call.command.as_str()),
                    ("/workspace", "grep"),
                    "{language}"
                );
                assert_eq!(
                    call.args[..2],
                    ["-rnE".to_owned(), expected.to_owned()],
                    "{language}"
                );
                assert_eq!(call.timeout, ExecTimeout::After(DEFAULT_EXEC_TIMEOUT));
            }
        }
    }

    mod get_file_globs_via_search_definition_args {
        use super::*;

        fn assert_includes(language: &str, globs: &[&str]) {
            let args = args(language);
            for glob in globs {
                assert!(
                    args.contains(&format!("--include={glob}")),
                    "{language}: {glob} in {args:?}"
                );
            }
        }

        #[test]
        fn typescript_includes_ts_and_tsx_globs() {
            assert_includes("typescript", &["*.ts", "*.tsx"]);
        }

        #[test]
        fn css_includes_css_scss_less_globs() {
            assert_includes("css", &["*.css", "*.scss", "*.less"]);
        }

        #[test]
        fn html_includes_html_htm_globs() {
            assert_includes("html", &["*.html", "*.htm"]);
        }

        #[test]
        fn unknown_language_uses_no_include_globs() {
            assert!(
                !args("brainfuck")
                    .iter()
                    .any(|arg| arg.starts_with("--include="))
            );
        }

        #[test]
        fn language_includes_correct_file_globs() {
            let cases: [(&str, &[&str]); 10] = [
                ("javascript", &["*.js", "*.jsx", "*.mjs", "*.cjs"]),
                ("python", &["*.py"]),
                ("rust", &["*.rs"]),
                ("go", &["*.go"]),
                ("java", &["*.java"]),
                ("kotlin", &["*.kt", "*.kts"]),
                ("ruby", &["*.rb"]),
                ("php", &["*.php"]),
                ("scss", &["*.css", "*.scss", "*.less"]),
                ("less", &["*.css", "*.scss", "*.less"]),
            ];
            for (language, globs) in cases {
                assert_includes(language, globs);
            }
        }

        #[test]
        fn excludes_non_source_directories_and_searches_the_workspace() {
            let args = args("rust");
            assert_eq!(
                args[args.len() - 6..],
                [
                    "--exclude-dir=node_modules",
                    "--exclude-dir=.git",
                    "--exclude-dir=dist",
                    "--exclude-dir=build",
                    "--exclude-dir=target",
                    "."
                ]
                .map(str::to_owned)
            );
        }
    }

    mod parse_grep_output_via_search_definition_result {
        use super::*;

        #[test]
        fn parses_valid_grep_output_lines() {
            let output = "src/foo.ts:10:function myFunc() {\nsrc/bar.ts:20:const myConst = 1\n";
            let (result, _) = search(exits(output, 0), "myFunc", "typescript");
            assert_eq!(
                result.unwrap(),
                vec![location("src/foo.ts", 10), location("src/bar.ts", 20)]
            );
        }

        #[test]
        fn strips_prefix_from_file_paths() {
            let (result, _) = search(
                exits("./src/foo.ts:5:class Foo {}\n", 0),
                "Foo",
                "typescript",
            );
            assert_eq!(result.unwrap(), vec![location("src/foo.ts", 5)]);
        }

        #[test]
        fn returns_empty_for_no_matches() {
            let (result, _) = search(exits("", 1), "nope", "typescript");
            assert_eq!(result.unwrap(), vec![]);
        }

        #[test]
        fn keeps_matches_when_grep_also_reports_unreadable_files() {
            let (result, _) = search(exits("a.rs:3:fn sym() {}\n", 2), "sym", "rust");
            assert_eq!(result.unwrap(), vec![location("a.rs", 3)]);
        }
    }

    mod error_paths {
        use super::*;

        /// TS: "returns empty when exec.start fails".
        #[test]
        fn fails_when_exec_start_fails() {
            let (result, _) = search(start_fails("no exec"), "sym", "typescript");
            assert!(matches!(result, Err(ExecError::Api(_))));
        }

        /// TS: "returns empty when exec.start throws".
        #[test]
        fn fails_when_the_stream_ends_without_a_result() {
            let (result, _) = search(
                FakeRun::Events(vec![Ok(ExecEvent::Stdout(b"a:1:x\n".to_vec()))]),
                "sym",
                "typescript",
            );
            assert!(matches!(result, Err(ExecError::EndedWithoutResult)));
        }

        /// TS: "returns empty on error event".
        #[test]
        fn fails_on_error_event() {
            let (result, _) = search(
                FakeRun::Events(vec![Err(ExecError::System("grep crashed".to_owned()))]),
                "sym",
                "typescript",
            );
            assert_eq!(result.unwrap_err().to_string(), "grep crashed");
        }
    }

    #[test]
    fn escapes_special_regex_chars_in_symbol_name() {
        let (result, exec) = search(exits("", 0), "foo.bar", "typescript");
        result.unwrap();
        assert!(exec.call(1).args[1].contains(r"foo\.bar"));
        assert_eq!(
            escape_regex(r"a.*+?^${}()|[]\b"),
            r"a\.\*\+\?\^\$\{\}\(\)\|\[\]\\b"
        );
    }
}
