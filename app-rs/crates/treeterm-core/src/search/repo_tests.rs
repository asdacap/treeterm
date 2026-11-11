//! Port of `utils/repoSearch.test.ts`, case by case. The TS `createControlledExecApi` becomes
//! [`FakeExec::push_controlled`]; `exec.kill` is the fake noticing a run dropped before it
//! finished; `cancel()` is dropping the stream.

use std::pin::Pin;

use futures::FutureExt;
use futures::channel::mpsc::UnboundedSender;

use super::*;
use crate::daemon::ApiError;
use crate::fakes::{FakeExec, start_fails};

type Events = Pin<Box<dyn Stream<Item = RepoSearchEvent> + Send>>;

fn start(exec: &Arc<FakeExec>, query: &str) -> Events {
    Box::pin(repo_search(exec.clone(), "/repo", query))
}

/// Everything the search has to report without waiting; `true` once it has ended.
fn drain(events: &mut Events) -> (Vec<RepoSearchEvent>, bool) {
    let mut drained = Vec::new();
    loop {
        match events.next().now_or_never() {
            Some(Some(event)) => drained.push(event),
            Some(None) => return (drained, true),
            None => return (drained, false),
        }
    }
}

fn stdout(tx: &UnboundedSender<Result<ExecEvent, ExecError>>, data: &str) {
    tx.unbounded_send(Ok(ExecEvent::Stdout(data.as_bytes().to_vec())))
        .unwrap();
}

fn stderr(tx: &UnboundedSender<Result<ExecEvent, ExecError>>, data: &str) {
    tx.unbounded_send(Ok(ExecEvent::Stderr(data.as_bytes().to_vec())))
        .unwrap();
}

fn exit(tx: &UnboundedSender<Result<ExecEvent, ExecError>>, code: i32) {
    tx.unbounded_send(Ok(ExecEvent::Exit { code })).unwrap();
}

fn system_error(tx: &UnboundedSender<Result<ExecEvent, ExecError>>, message: &str) {
    tx.unbounded_send(Err(ExecError::System(message.to_owned())))
        .unwrap();
}

fn hit(file_path: &str, line_number: u64, column: u64, preview: &str) -> RepoHit {
    RepoHit {
        file_path: file_path.to_owned(),
        line_number,
        column,
        preview: preview.to_owned(),
    }
}

fn hits_of(event: &RepoSearchEvent) -> &[RepoHit] {
    match event {
        RepoSearchEvent::Hits(hits) => hits,
        other => panic!("expected hits, got {other:?}"),
    }
}

fn assert_done(event: &RepoSearchEvent, expected_tool: RepoSearchTool, expected: Completion) {
    match event {
        RepoSearchEvent::Done { tool, completion } => {
            assert_eq!((*tool, *completion), (expected_tool, expected))
        }
        other => panic!("expected done, got {other:?}"),
    }
}

fn failure(event: &RepoSearchEvent) -> &RepoSearchError {
    match event {
        RepoSearchEvent::Failed(error) => error,
        other => panic!("expected a failure, got {other:?}"),
    }
}

mod parse_vimgrep_line {
    use super::*;

    #[test]
    fn parses_path_line_column_and_text_keeping_colons_in_the_text() {
        assert_eq!(
            parse_vimgrep_line("./src/a.ts:12:5:const x = { a: 1 }").unwrap(),
            hit("src/a.ts", 12, 5, "const x = { a: 1 }")
        );
    }

    #[test]
    fn keeps_paths_without_a_prefix_as_they_are() {
        assert_eq!(
            parse_vimgrep_line("src/a.ts:1:1:x").unwrap().file_path,
            "src/a.ts"
        );
    }

    #[test]
    fn returns_an_error_for_unexpected_output() {
        assert!(matches!(
            parse_vimgrep_line("garbage"),
            Err(RepoSearchError::UnexpectedOutput { .. })
        ));
    }
}

mod start_repo_search {
    use super::*;

    #[test]
    fn runs_rg_with_smart_case_fixed_string_search() {
        let exec = FakeExec::new(vec![]);
        let _tx = exec.push_controlled();
        let mut events = start(&exec, "foo");
        assert!(drain(&mut events).0.is_empty());
        let call = exec.call(1);
        assert_eq!((call.cwd.as_str(), call.command.as_str()), ("/repo", "rg"));
        assert_eq!(
            call.args,
            vec![
                "--vimgrep",
                "--fixed-strings",
                "--smart-case",
                "--max-columns",
                "300",
                "--max-columns-preview",
                "--",
                "foo",
                "."
            ]
        );
        assert_eq!(call.timeout, ExecTimeout::After(REPO_SEARCH_TIMEOUT));
    }

    #[test]
    fn streams_hits_as_complete_lines_arrive_across_chunk_boundaries() {
        let exec = FakeExec::new(vec![]);
        let tx = exec.push_controlled();
        let mut events = start(&exec, "foo");

        stdout(&tx, "./a.ts:1:1:foo\n./b.t");
        let (first, ended) = drain(&mut events);
        assert!(!ended);
        assert_eq!(first.len(), 1);
        assert_eq!(hits_of(&first[0]), [hit("a.ts", 1, 1, "foo")]);

        stdout(&tx, "s:2:3:foo");
        exit(&tx, 0);
        let (rest, ended) = drain(&mut events);
        assert!(ended);
        assert_eq!(rest.len(), 2);
        assert_eq!(hits_of(&rest[0]), [hit("b.ts", 2, 3, "foo")]);
        assert_done(&rest[1], RepoSearchTool::Ripgrep, Completion::Complete);
    }

    #[test]
    fn treats_exit_code_1_as_no_matches() {
        let exec = FakeExec::new(vec![]);
        let tx = exec.push_controlled();
        let mut events = start(&exec, "foo");
        exit(&tx, 1);
        let (drained, ended) = drain(&mut events);
        assert!(ended);
        assert_eq!(drained.len(), 1);
        assert_done(&drained[0], RepoSearchTool::Ripgrep, Completion::Complete);
    }

    #[test]
    fn reports_other_exit_codes_as_errors_with_stderr() {
        let exec = FakeExec::new(vec![]);
        let tx = exec.push_controlled();
        let mut events = start(&exec, "foo");
        stderr(&tx, "bad regex\n");
        exit(&tx, 2);
        let (drained, ended) = drain(&mut events);
        assert!(ended);
        assert_eq!(
            failure(&drained[0]).to_string(),
            "rg exited with code 2: bad regex"
        );
    }

    #[test]
    fn reports_unparseable_output_as_an_error_and_stops() {
        let exec = FakeExec::new(vec![]);
        let tx = exec.push_controlled();
        let mut events = start(&exec, "foo");
        stdout(&tx, "garbage\n");
        let (drained, ended) = drain(&mut events);
        assert!(ended);
        assert_eq!(drained.len(), 1);
        assert_eq!(
            failure(&drained[0]).to_string(),
            "Unexpected search output: garbage"
        );
        assert!(exec.killed(1));
    }

    #[test]
    fn falls_back_to_git_grep_when_rg_cannot_be_spawned() {
        for (query, git_args) in [
            (
                "foo",
                vec!["grep", "-n", "--column", "-I", "-F", "-i", "-e", "foo"],
            ),
            (
                "Foo",
                vec!["grep", "-n", "--column", "-I", "-F", "-e", "Foo"],
            ),
        ] {
            let exec = FakeExec::new(vec![]);
            let rg = exec.push_controlled();
            let git = exec.push_controlled();
            let mut events = start(&exec, query);
            system_error(&rg, "spawn error: No such file or directory (os error 2)");
            assert!(drain(&mut events).0.is_empty());

            let call = exec.call(2);
            assert_eq!((call.cwd.as_str(), call.command.as_str()), ("/repo", "git"));
            assert_eq!(call.args, git_args);
            assert_eq!(call.timeout, ExecTimeout::After(REPO_SEARCH_TIMEOUT));

            stdout(&git, &format!("a.ts:4:2:{query}\n"));
            exit(&git, 0);
            let (drained, ended) = drain(&mut events);
            assert!(ended);
            assert_eq!(hits_of(&drained[0]), [hit("a.ts", 4, 2, query)]);
            assert_done(&drained[1], RepoSearchTool::GitGrep, Completion::Complete);
        }
    }

    #[test]
    fn reports_other_exec_errors_including_from_git_grep() {
        let exec = FakeExec::new(vec![]);
        let rg = exec.push_controlled();
        let git = exec.push_controlled();
        let mut events = start(&exec, "foo");
        system_error(&rg, "spawn error: missing");
        assert!(drain(&mut events).0.is_empty());
        system_error(&git, "spawn error: missing");
        let (drained, ended) = drain(&mut events);
        assert!(ended);
        assert_eq!(drained.len(), 1);
        assert_eq!(failure(&drained[0]).to_string(), "spawn error: missing");
    }

    #[test]
    fn reports_a_failed_start() {
        let exec = FakeExec::new(vec![start_fails("no connection")]);
        let mut events = start(&exec, "foo");
        let (drained, ended) = drain(&mut events);
        assert!(ended);
        assert!(failure(&drained[0]).to_string().contains("no connection"));
    }

    #[test]
    fn reports_a_rejected_start() {
        let exec = FakeExec::new(vec![]);
        let tx = exec.push_controlled();
        tx.unbounded_send(Err(ExecError::Api(ApiError::TaskLost(crate::rt::TaskLost))))
            .unwrap();
        let mut events = start(&exec, "foo");
        let (drained, ended) = drain(&mut events);
        assert!(ended);
        assert!(matches!(
            failure(&drained[0]),
            RepoSearchError::Exec(ExecError::Api(_))
        ));
    }

    #[test]
    fn stops_at_the_hit_cap_kills_the_exec_and_reports_truncation() {
        let exec = FakeExec::new(vec![]);
        let tx = exec.push_controlled();
        let mut events = start(&exec, "foo");
        let lines: String = (1..=MAX_REPO_HITS + 5)
            .map(|i| format!("a.ts:{i}:1:foo\n"))
            .collect();
        stdout(&tx, &lines);

        let (drained, ended) = drain(&mut events);
        assert!(ended);
        let hits: usize = drained
            .iter()
            .map(|event| match event {
                RepoSearchEvent::Hits(hits) => hits.len(),
                RepoSearchEvent::Done { .. } | RepoSearchEvent::Failed(_) => 0,
            })
            .sum();
        assert_eq!(hits, MAX_REPO_HITS);
        assert_done(
            drained.last().unwrap(),
            RepoSearchTool::Ripgrep,
            Completion::Truncated,
        );
        assert!(exec.killed(1));
    }

    #[test]
    fn cancel_kills_the_running_exec_and_delivers_nothing_more() {
        let exec = FakeExec::new(vec![]);
        let tx = exec.push_controlled();
        let mut events = start(&exec, "foo");
        assert!(drain(&mut events).0.is_empty());
        drop(events);
        assert!(exec.killed(1));
        assert!(
            tx.unbounded_send(Ok(ExecEvent::Stdout(b"a.ts:1:1:foo\n".to_vec())))
                .is_err()
        );
    }

    /// TS killed the exec once its start resolved; here nothing starts until the stream is
    /// first polled.
    #[test]
    fn cancel_before_the_exec_has_started_kills_it_once_it_starts() {
        let exec = FakeExec::new(vec![]);
        drop(start(&exec, "foo"));
        assert!(exec.calls().is_empty());
    }

    #[test]
    fn cancel_after_the_search_finished_does_not_kill_anything() {
        let exec = FakeExec::new(vec![]);
        let tx = exec.push_controlled();
        let mut events = start(&exec, "foo");
        exit(&tx, 0);
        assert!(drain(&mut events).1);
        drop(events);
        assert!(!exec.killed(1));
    }

    #[test]
    fn cancel_before_a_failed_start_reports_nothing() {
        let exec = FakeExec::new(vec![start_fails("no connection")]);
        drop(start(&exec, "foo"));
        assert!(exec.calls().is_empty());
    }
}

mod workspace_path_helpers {
    use super::*;

    #[test]
    fn joins_a_relative_hit_path_onto_the_workspace() {
        assert_eq!(to_workspace_absolute("/repo", "src/a.ts"), "/repo/src/a.ts");
    }

    #[test]
    fn strips_the_workspace_prefix_leaving_relative_paths_alone() {
        assert_eq!(to_workspace_relative("/repo", "/repo/src/a.ts"), "src/a.ts");
        assert_eq!(to_workspace_relative("/repo", "src/a.ts"), "src/a.ts");
        assert_eq!(
            to_workspace_relative("/repo", "/repository/a.ts"),
            "/repository/a.ts"
        );
    }
}
