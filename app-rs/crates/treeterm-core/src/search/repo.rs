//! Search the whole worktree with ripgrep, or `git grep` when rg is not installed on the
//! daemon's host (port of `utils/repoSearch.ts`). Hits stream in as the command prints
//! them; repos can be big, so the search is capped and dropping it cancels the command.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use futures::stream::{BoxStream, Stream, StreamExt};
use regex::Regex;

use crate::exec::{ExecApi, ExecError, ExecEvent, ExecRequest, ExecTimeout};

pub const MAX_REPO_HITS: usize = 2000;
/// Long: the caller cancels the search when it is no longer wanted.
pub const REPO_SEARCH_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoHit {
    /// Relative to the searched directory.
    pub file_path: String,
    pub line_number: u64,
    pub column: u64,
    pub preview: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoSearchTool {
    Ripgrep,
    GitGrep,
}

impl RepoSearchTool {
    pub fn as_str(self) -> &'static str {
        match self {
            RepoSearchTool::Ripgrep => "rg",
            RepoSearchTool::GitGrep => "git grep",
        }
    }

    fn command(self) -> &'static str {
        match self {
            RepoSearchTool::Ripgrep => "rg",
            RepoSearchTool::GitGrep => "git",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    Complete,
    /// Stopped at [`MAX_REPO_HITS`].
    Truncated,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum RepoSearchError {
    #[error("Unexpected search output: {line}")]
    UnexpectedOutput { line: String },
    #[error("{command} exited with code {code}: {stderr}")]
    Exit {
        command: &'static str,
        code: i32,
        stderr: String,
    },
    #[error(transparent)]
    Exec(#[from] ExecError),
}

/// What a search reports. `Done` or `Failed` is the last item.
#[derive(Debug, Clone)]
pub enum RepoSearchEvent {
    Hits(Vec<RepoHit>),
    Done {
        tool: RepoSearchTool,
        completion: Completion,
    },
    Failed(RepoSearchError),
}

/// `path:line:col:text`, the format of both `rg --vimgrep` and `git grep -n --column`.
static VIMGREP_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+?):(\d+):(\d+):(.*)$").expect("valid regex"));

pub fn parse_vimgrep_line(line: &str) -> Result<RepoHit, RepoSearchError> {
    let unexpected = || RepoSearchError::UnexpectedOutput {
        line: line.to_owned(),
    };
    let captures = VIMGREP_LINE.captures(line).ok_or_else(unexpected)?;
    let raw_path = &captures[1];
    Ok(RepoHit {
        file_path: raw_path.strip_prefix("./").unwrap_or(raw_path).to_owned(),
        line_number: captures[2].parse().map_err(|_| unexpected())?,
        column: captures[3].parse().map_err(|_| unexpected())?,
        preview: captures[4].to_owned(),
    })
}

/// Both tools use smart case: an all-lowercase query matches case-insensitively.
fn args_for(tool: RepoSearchTool, query: &str) -> Vec<String> {
    let args: Vec<&str> = match tool {
        RepoSearchTool::Ripgrep => vec![
            "--vimgrep",
            "--fixed-strings",
            "--smart-case",
            "--max-columns",
            "300",
            "--max-columns-preview",
            "--",
            query,
            ".",
        ],
        RepoSearchTool::GitGrep => {
            let mut args = vec!["grep", "-n", "--column", "-I", "-F"];
            if query.to_lowercase() == query {
                args.push("-i");
            }
            args.extend(["-e", query]);
            args
        }
    };
    args.into_iter().map(str::to_owned).collect()
}

/// The daemon reports a binary it could not start this way (`exec_manager.rs`).
fn is_spawn_error(error: &ExecError) -> bool {
    matches!(error, ExecError::System(message) if message.contains("spawn error"))
}

struct Search {
    exec: Arc<dyn ExecApi>,
    cwd: String,
    query: String,
    tool: RepoSearchTool,
    /// The running command; dropping it kills the command.
    events: Option<BoxStream<'static, Result<ExecEvent, ExecError>>>,
    /// Stdout after the last complete line.
    partial: Vec<u8>,
    stderr: Vec<u8>,
    hit_count: usize,
    pending: VecDeque<RepoSearchEvent>,
    finished: bool,
}

/// Whether to keep reading after a batch of lines.
enum Flow {
    Continue,
    Stopped,
}

impl Search {
    fn finish(&mut self, event: RepoSearchEvent) {
        self.events = None;
        self.pending.push_back(event);
        self.finished = true;
    }

    fn emit_lines(&mut self, lines: Vec<Vec<u8>>) -> Flow {
        let mut hits = Vec::new();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            match parse_vimgrep_line(&String::from_utf8_lossy(&line)) {
                Ok(hit) => hits.push(hit),
                Err(error) => {
                    self.finish(RepoSearchEvent::Failed(error));
                    return Flow::Stopped;
                }
            }
            if self.hit_count + hits.len() >= MAX_REPO_HITS {
                break;
            }
        }
        self.hit_count += hits.len();
        if !hits.is_empty() {
            self.pending.push_back(RepoSearchEvent::Hits(hits));
        }
        if self.hit_count >= MAX_REPO_HITS {
            let tool = self.tool;
            self.finish(RepoSearchEvent::Done {
                tool,
                completion: Completion::Truncated,
            });
            return Flow::Stopped;
        }
        Flow::Continue
    }

    async fn step(&mut self) {
        let (exec, cwd, query, tool) = (&self.exec, &self.cwd, &self.query, self.tool);
        let events = self.events.get_or_insert_with(|| {
            exec.start(ExecRequest {
                cwd: cwd.clone(),
                command: tool.command().to_owned(),
                args: args_for(tool, query),
                env: HashMap::new(),
                timeout: ExecTimeout::After(REPO_SEARCH_TIMEOUT),
            })
        });
        match events.next().await {
            Some(Ok(ExecEvent::Stdout(data))) => {
                self.partial.extend_from_slice(&data);
                let complete = match self.partial.iter().rposition(|byte| *byte == b'\n') {
                    Some(last) => self.partial.drain(..=last).collect::<Vec<u8>>(),
                    None => return,
                };
                let lines = complete
                    .split(|byte| *byte == b'\n')
                    .map(<[u8]>::to_vec)
                    .collect();
                match self.emit_lines(lines) {
                    // Stopped or not, what follows comes from `pending`.
                    Flow::Continue | Flow::Stopped => {}
                }
            }
            Some(Ok(ExecEvent::Stderr(data))) => self.stderr.extend_from_slice(&data),
            Some(Ok(ExecEvent::Exit { code })) => {
                self.events = None;
                let rest = std::mem::take(&mut self.partial);
                if let Flow::Stopped = self.emit_lines(vec![rest]) {
                    return;
                }
                // Both tools exit 1 when nothing matched.
                let event = match code {
                    0 | 1 => RepoSearchEvent::Done {
                        tool,
                        completion: Completion::Complete,
                    },
                    _ => RepoSearchEvent::Failed(RepoSearchError::Exit {
                        command: tool.command(),
                        code,
                        stderr: String::from_utf8_lossy(&self.stderr).trim().to_owned(),
                    }),
                };
                self.finish(event);
            }
            Some(Err(error)) if tool == RepoSearchTool::Ripgrep && is_spawn_error(&error) => {
                self.events = None;
                self.tool = RepoSearchTool::GitGrep;
                self.partial.clear();
                self.stderr.clear();
            }
            Some(Err(error)) => self.finish(RepoSearchEvent::Failed(error.into())),
            None => self.finish(RepoSearchEvent::Failed(
                ExecError::EndedWithoutResult.into(),
            )),
        }
    }
}

/// Search for `query` under `cwd`. The command starts on the first poll; the stream ends
/// after its `Done` or `Failed` event, and dropping it earlier kills the command.
pub fn repo_search(
    exec: Arc<dyn ExecApi>,
    cwd: &str,
    query: &str,
) -> impl Stream<Item = RepoSearchEvent> + Send + 'static {
    let search = Search {
        exec,
        cwd: cwd.to_owned(),
        query: query.to_owned(),
        tool: RepoSearchTool::Ripgrep,
        events: None,
        partial: Vec::new(),
        stderr: Vec::new(),
        hit_count: 0,
        pending: VecDeque::new(),
        finished: false,
    };
    futures::stream::unfold(search, |mut search| async move {
        loop {
            if let Some(event) = search.pending.pop_front() {
                return Some((event, search));
            }
            if search.finished {
                return None;
            }
            search.step().await;
        }
    })
}

/// Search hits are relative to the workspace; file tree entries (and so a file browser
/// tab's selected path) are absolute.
pub fn to_workspace_absolute(workspace_path: &str, relative_path: &str) -> String {
    format!("{workspace_path}/{relative_path}")
}

pub fn to_workspace_relative<'a>(workspace_path: &str, file_path: &'a str) -> &'a str {
    file_path
        .strip_prefix(workspace_path)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(file_path)
}

#[cfg(test)]
#[path = "repo_tests.rs"]
mod tests;
