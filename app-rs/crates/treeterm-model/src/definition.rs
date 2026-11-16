//! Go to definition, shared by the Files viewer and the Editor tab (`definitionProvider` in
//! `monaco-config.ts`, which both `FileViewer` and `FileEditor` wire): grep the workspace for
//! a definition of the word under the cursor ([`search::search_definition`]), in the
//! languages Electron registers the provider for. One result is gone to, several are
//! offered to choose from.
//!
//! [`DefinitionProbe`] answers the Cmd/Ctrl+hover underline Monaco draws under a word that
//! has a definition.

use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;

use futures::FutureExt as _;
use futures::future::Shared;
use gpui_kit::{App, Task};
use treeterm_core::exec::{ExecApi, ExecError};
use treeterm_core::files::detect_monaco_language;
use treeterm_core::search::{self, DefinitionLocation};

/// Monaco languages Electron registers its grep definition provider for.
pub const DEFINITION_LANGUAGES: [&str; 16] = [
    "typescript", "javascript", "python", "rust", "go", "java", "kotlin", "ruby", "php", "c", "cpp", "css", "scss",
    "less", "html", "shell",
];

/// Whether a file of `path` has go to definition (Monaco shows the context menu's "Go to
/// Definition" only then).
pub fn has_definitions(path: &str) -> bool {
    DEFINITION_LANGUAGES.contains(&detect_monaco_language(path))
}

/// Where a go to definition stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionLookup {
    Idle,
    Searching { symbol: String },
    /// Several candidates: the user picks one.
    Choose { symbol: String, locations: Vec<DefinitionLocation> },
    NotFound { symbol: String },
    /// No definition search for this file's language (Electron registers none).
    Unavailable { language: &'static str },
    Failed { symbol: String, error: String },
}

/// A definition search to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub symbol: String,
    pub language: &'static str,
}

/// How a go to definition at a byte offset starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Begin {
    /// Not on a word: nothing to look up.
    Idle,
    Unavailable { language: &'static str },
    Search(Query),
}

/// How a go to definition at byte `offset` of `path`'s `text` starts.
pub fn begin(path: &str, text: &str, offset: usize) -> Begin {
    let symbol = search::word_at(text, offset);
    let language = detect_monaco_language(path);
    if symbol.is_empty() {
        return Begin::Idle;
    }
    if !DEFINITION_LANGUAGES.contains(&language) {
        return Begin::Unavailable { language };
    }
    Begin::Search(Query { symbol: symbol.to_owned(), language })
}

/// How a definition lookup shows a lone definition: Go to Definition goes there, Peek
/// Definition lists it, as either lists several (Monaco's peek widget, which TreeTerm shows
/// as a list over the editor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigate {
    Go,
    Peek,
}

/// What a finished search leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settled {
    /// The one definition: go there.
    Go(DefinitionLocation),
    /// None, several, or the search failed.
    Show(DefinitionLookup),
}

/// What the search for `symbol` found leads to, looked up to `navigate` there.
pub fn settle(symbol: String, result: Result<Vec<DefinitionLocation>, ExecError>, navigate: Navigate) -> Settled {
    match result {
        Err(error) => Settled::Show(DefinitionLookup::Failed { symbol, error: error.to_string() }),
        Ok(locations) => match <[DefinitionLocation; 1]>::try_from(locations) {
            Ok([location]) => match navigate {
                Navigate::Go => Settled::Go(location),
                Navigate::Peek => Settled::Show(DefinitionLookup::Choose { symbol, locations: vec![location] }),
            },
            Err(locations) if locations.is_empty() => Settled::Show(DefinitionLookup::NotFound { symbol }),
            Err(locations) => Settled::Show(DefinitionLookup::Choose { symbol, locations }),
        },
    }
}

/// Where `query`'s symbol is defined under `root`.
pub(crate) async fn run(
    exec: Arc<dyn ExecApi>,
    root: String,
    query: Query,
) -> Result<Vec<DefinitionLocation>, ExecError> {
    search::search_definition(exec.as_ref(), &root, &query.symbol, query.language).await
}

/// Whether a word has a definition, once its search ends (a failed search counts as none;
/// going to the definition shows why it failed).
pub type HasDefinition = Shared<Task<bool>>;

/// The Cmd/Ctrl+hover underline of one file: whether grep finds a definition of the word
/// under the pointer. Moving over the same word again joins its search rather than
/// starting another, as Monaco keeps one per word.
pub struct DefinitionProbe {
    exec: Arc<dyn ExecApi>,
    root: String,
    path: String,
    /// The last word searched for, and its search.
    last: RefCell<Option<(String, HasDefinition)>>,
}

impl DefinitionProbe {
    pub fn new(exec: Arc<dyn ExecApi>, root: String, path: String) -> Self {
        Self { exec, root, path, last: RefCell::new(None) }
    }

    /// The word at byte `offset` of `text` and whether it has a definition; `None` when
    /// there is nothing to look up there.
    pub fn probe(&self, text: &str, offset: usize, cx: &App) -> Option<(Range<usize>, HasDefinition)> {
        let Begin::Search(query) = begin(&self.path, text, offset) else {
            return None;
        };
        let word = search::word_range_at(text, offset);
        let mut last = self.last.borrow_mut();
        if let Some((symbol, found)) = last.as_ref()
            && *symbol == query.symbol
        {
            return Some((word, found.clone()));
        }
        let (key, symbol) = (query.symbol.clone(), query.symbol.clone());
        let search = run(self.exec.clone(), self.root.clone(), query);
        let found = cx
            .foreground_executor()
            .spawn(async move {
                match search.await {
                    Ok(locations) => !locations.is_empty(),
                    Err(error) => {
                        tracing::warn!("definition search for {symbol} failed: {error}");
                        false
                    }
                }
            })
            .shared();
        *last = Some((key, found.clone()));
        Some((word, found))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(file_path: &str, line_number: u64) -> DefinitionLocation {
        DefinitionLocation { file_path: file_path.to_owned(), line_number }
    }

    #[test]
    fn a_lookup_starts_on_a_word_of_a_language_with_definitions() {
        assert_eq!(begin("/w/a.rs", "fn  main", 3), Begin::Idle);
        assert_eq!(begin("/w/notes.txt", "needle", 2), Begin::Unavailable { language: "plaintext" });
        assert_eq!(
            begin("/w/a.rs", "fn main", 4),
            Begin::Search(Query { symbol: "main".into(), language: "rust" })
        );
    }

    #[test]
    fn one_result_is_gone_to_and_others_are_shown() {
        assert_eq!(settle("x".into(), Ok(vec![location("a.rs", 3)]), Navigate::Go), Settled::Go(location("a.rs", 3)));
        assert_eq!(
            settle("x".into(), Ok(vec![]), Navigate::Go),
            Settled::Show(DefinitionLookup::NotFound { symbol: "x".into() })
        );
        let two = vec![location("a.rs", 3), location("b.rs", 1)];
        for navigate in [Navigate::Go, Navigate::Peek] {
            assert_eq!(
                settle("x".into(), Ok(two.clone()), navigate),
                Settled::Show(DefinitionLookup::Choose { symbol: "x".into(), locations: two.clone() })
            );
        }
        let failed = settle("x".into(), Err(ExecError::System("grep: not found".into())), Navigate::Go);
        assert!(
            matches!(&failed, Settled::Show(DefinitionLookup::Failed { symbol, error }) if symbol == "x" && error.contains("grep")),
            "{failed:?}"
        );
    }

    #[test]
    fn peek_lists_a_lone_definition() {
        assert_eq!(
            settle("x".into(), Ok(vec![location("a.rs", 3)]), Navigate::Peek),
            Settled::Show(DefinitionLookup::Choose { symbol: "x".into(), locations: vec![location("a.rs", 3)] })
        );
        assert_eq!(
            settle("x".into(), Ok(vec![]), Navigate::Peek),
            Settled::Show(DefinitionLookup::NotFound { symbol: "x".into() })
        );
    }

    #[test]
    fn only_registered_languages_have_definitions() {
        assert!(has_definitions("/w/a.ts"));
        assert!(has_definitions("/w/run.sh"));
        assert!(!has_definitions("/w/notes.txt"));
        assert!(!has_definitions("/w/README.md"));
    }
}
