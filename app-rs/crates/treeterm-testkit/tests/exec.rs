//! `ExecStream` against a real in-process daemon.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use futures::StreamExt;
use treeterm_core::exec::{
    CommandOutput, ExecClient, ExecError, ExecEvent, ExecRequest, ExecTimeout, collect_output,
};
use treeterm_core::rt::Rt;
use treeterm_core::transport::{connect_unix, daemon_client};
use treeterm_testkit::TestDaemon;

fn rt() -> Rt {
    Rt::new(tokio::runtime::Handle::current())
}

async fn client(daemon: &TestDaemon) -> ExecClient {
    let channel = connect_unix(daemon.socket_path()).await.unwrap();
    ExecClient::new(rt(), daemon_client(channel))
}

fn request(daemon: &TestDaemon, command: &str, args: &[&str]) -> ExecRequest {
    ExecRequest {
        cwd: daemon.root().to_string_lossy().into_owned(),
        command: command.to_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        env: HashMap::new(),
        timeout: ExecTimeout::After(Duration::from_secs(10)),
    }
}

/// Fail instead of hanging when a command never finishes.
async fn within<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("timed out")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_collects_stdout_and_the_exit_code() {
    let daemon = TestDaemon::start();
    let exec = client(&daemon).await;
    let output = within(exec.run(request(&daemon, "echo", &["hello", "world"])))
        .await
        .unwrap();
    assert_eq!(
        output,
        CommandOutput {
            exit_code: 0,
            stdout: b"hello world\n".to_vec(),
            stderr: Vec::new(),
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_nonzero_exit_is_a_result_not_an_error() {
    let daemon = TestDaemon::start();
    let exec = client(&daemon).await;
    let output = within(exec.run(request(&daemon, "sh", &["-c", "echo oops >&2; exit 3"])))
        .await
        .unwrap();
    assert_eq!(output.exit_code, 3);
    assert_eq!(output.stderr, b"oops\n");
    assert!(output.stdout.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn env_and_cwd_apply_to_the_command() {
    let daemon = TestDaemon::start();
    let exec = client(&daemon).await;
    let mut with_env = request(&daemon, "sh", &["-c", "echo \"$TT_EXEC_VAR\"; pwd -P"]);
    with_env
        .env
        .insert("TT_EXEC_VAR".to_owned(), "bar".to_owned());
    let output = within(exec.run(with_env)).await.unwrap();
    let root = daemon.root().canonicalize().unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("bar\n{}\n", root.display())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdin_reaches_the_command_and_closing_it_is_eof() {
    let daemon = TestDaemon::start();
    let exec = client(&daemon).await;
    let process = exec.exec(request(&daemon, "cat", &[]));
    // Several stdin messages' worth, so chunking and ordering both matter.
    let input: Vec<u8> = (0..200_000u32).map(|i| b'a' + (i % 26) as u8).collect();
    process.input.write_stdin(&input).unwrap();
    process.input.write_stdin(b"-end").unwrap();
    process.input.close_stdin();
    let output = within(collect_output(process.events)).await.unwrap();
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout.len(), input.len() + 4);
    assert!(output.stdout.starts_with(&input));
    assert!(output.stdout.ends_with(b"-end"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_timeout_terminates_the_command() {
    let daemon = TestDaemon::start();
    let exec = client(&daemon).await;
    let mut slow = request(&daemon, "sleep", &["30"]);
    slow.timeout = ExecTimeout::After(Duration::from_millis(200));
    let started = Instant::now();
    let output = within(exec.run(slow)).await.unwrap();
    assert_eq!(output.exit_code, -1, "killed by SIGTERM: no exit code");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_signal_stops_the_command() {
    let daemon = TestDaemon::start();
    let exec = client(&daemon).await;
    let mut long = request(&daemon, "sh", &["-c", "echo ready; exec sleep 30"]);
    long.timeout = ExecTimeout::Never;
    let mut process = exec.exec(long);
    let first = within(process.events.next()).await.unwrap().unwrap();
    assert_eq!(first, ExecEvent::Stdout(b"ready\n".to_vec()));
    process.input.signal(libc::SIGTERM).unwrap();
    let rest: Vec<_> = within(process.events.collect::<Vec<_>>()).await;
    assert!(
        matches!(&rest[..], [Ok(ExecEvent::Exit { code: -1 })]),
        "{rest:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_that_cannot_start_is_a_system_error() {
    let daemon = TestDaemon::start();
    let exec = client(&daemon).await;
    let missing = within(exec.run(request(&daemon, "/no/such/binary", &[]))).await;
    assert!(
        matches!(&missing, Err(ExecError::System(message)) if message.contains("spawn error")),
        "{missing:?}"
    );

    let mut bad_cwd = request(&daemon, "echo", &["hi"]);
    bad_cwd.cwd = "/no/such/dir".to_owned();
    let result = within(exec.run(bad_cwd)).await;
    assert!(matches!(result, Err(ExecError::System(_))), "{result:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_events_kills_the_command() {
    let daemon = TestDaemon::start();
    let exec = client(&daemon).await;
    let marker = daemon.root().join("marker");
    let script = format!("echo started; sleep 1; touch '{}'", marker.display());
    let mut process = exec.exec(request(&daemon, "sh", &["-c", &script]));
    let first = within(process.events.next()).await.unwrap().unwrap();
    assert_eq!(first, ExecEvent::Stdout(b"started\n".to_vec()));
    drop(process);
    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert!(
        !marker.exists(),
        "the command ran on after it was cancelled"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn input_fails_once_the_command_is_done() {
    let daemon = TestDaemon::start();
    let exec = client(&daemon).await;
    let mut process = exec.exec(request(&daemon, "true", &[]));
    let events: Vec<_> = within((&mut process.events).collect::<Vec<_>>()).await;
    assert!(
        matches!(&events[..], [Ok(ExecEvent::Exit { code: 0 })]),
        "{events:?}"
    );
    // The call is gone once the result arrived; give the transport a moment to notice.
    let closed = within(async {
        loop {
            if process.input.write_stdin(b"late").is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    closed.await;
}
