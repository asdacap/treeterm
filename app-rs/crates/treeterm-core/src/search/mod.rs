//! Searching diffs, files, the repository and definitions.

pub mod definition;
pub mod diff;
pub mod line_diff;
pub mod repo;

pub use definition::{DefinitionLocation, definition_pattern, file_globs, search_definition};
pub use diff::{
    DiffSearchHit, DiffSearchOptions, DiffSearchSide, DiffStyle, LineMatcher, SearchableDiff,
    TextSearchHit, UnchangedRegions, find_in_text, find_lines_in_diff, word_at,
};
pub use line_diff::{Hunk, HunkBlock, LineDiff, Whitespace, line_diff};
pub use repo::{
    Completion, MAX_REPO_HITS, REPO_SEARCH_TIMEOUT, RepoHit, RepoSearchError, RepoSearchEvent,
    RepoSearchTool, parse_vimgrep_line, repo_search, to_workspace_absolute, to_workspace_relative,
};
