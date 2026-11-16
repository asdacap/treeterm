//! PTY attachments against a real in-process daemon.

use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt;
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::pty::{
    AttachKind, CreatePty, PtyAttachment, PtyClosed, PtyEvent, STREAM_DETACHED, STREAM_ENDED,
};
use treeterm_core::rt::Rt;
use treeterm_testkit::TestDaemon;

fn rt() -> Rt {
    Rt::new(tokio::runtime::Handle::current())
}

/// A PTY running the login shell with an empty config dir, so the user's dotfiles don't
/// slow the test down or print things.
async fn spawn_shell(conn: &DaemonConnection, daemon: &TestDaemon) -> String {
    let mut env = HashMap::new();
    env.insert("ZDOTDIR".to_owned(), daemon.root().to_string_lossy().into_owned());
    env.insert("PS1".to_owned(), "$ ".to_owned());
    conn.create_pty(CreatePty {
        cwd: daemon.root().to_string_lossy().into_owned(),
        env,
        size: Some((80, 24)),
        ..CreatePty::default()
    })
    .await
    .unwrap()
    .unwrap()
}

/// Collect output until `done` holds for it, failing after a generous timeout.
async fn read_until(
    attachment: &mut PtyAttachment,
    mut done: impl FnMut(&[PtyEvent]) -> bool,
) -> Vec<PtyEvent> {
    let mut seen = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = attachment.events.next().await {
            seen.push(event.expect("pty stream error"));
            if done(&seen) {
                return;
            }
        }
        panic!("pty stream ended early: {seen:?}");
    })
    .await;
    assert!(result.is_ok(), "timed out; saw {seen:?}");
    seen
}

fn output_text(events: &[PtyEvent]) -> String {
    let mut bytes = Vec::new();
    for event in events {
        if let PtyEvent::Data(data) = event {
            bytes.extend_from_slice(data);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn attach_replays_then_streams_live_output_and_takes_input() {
    let daemon = TestDaemon::start();
    let conn = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();
    let pty = spawn_shell(&conn, &daemon).await;

    let mut attachment = conn.attach_pty(&pty, AttachKind::Terminal);
    let replay = read_until(&mut attachment, |seen| seen.contains(&PtyEvent::ReplayEnd)).await;
    assert_eq!(replay.first(), Some(&PtyEvent::ReplayStart));

    attachment.input.focus().unwrap();
    // The marker is computed by the shell, so the echoed command line never matches it.
    attachment.input.write(b"echo tt-marker-$((40+2))\r").await.unwrap();
    read_until(&mut attachment, |seen| output_text(seen).contains("tt-marker-42")).await;

    conn.kill_pty(&pty).await.unwrap().unwrap();
    read_until(&mut attachment, |seen| {
        matches!(seen.last(), Some(PtyEvent::Exit { .. }))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resize_request_is_echoed_with_the_applied_size() {
    let daemon = TestDaemon::start();
    let conn = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();
    let pty = spawn_shell(&conn, &daemon).await;

    let mut attachment = conn.attach_pty(&pty, AttachKind::Terminal);
    read_until(&mut attachment, |seen| seen.contains(&PtyEvent::ReplayEnd)).await;
    attachment.input.resize(100, 30).unwrap();
    read_until(&mut attachment, |seen| {
        seen.contains(&PtyEvent::Resize { cols: 100, rows: 30 })
    })
    .await;

    let sessions = conn.list_pty_sessions().await.unwrap().unwrap();
    let info = sessions.iter().find(|s| s.id == pty).expect("pty listed");
    assert_eq!((info.cols, info.rows), (100, 30));
    conn.kill_pty(&pty).await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_same_handle_returns_the_live_pty() {
    let daemon = TestDaemon::start();
    let conn = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();
    let params = CreatePty {
        cwd: daemon.root().to_string_lossy().into_owned(),
        handle: Some("tab-handle-1".to_owned()),
        ..CreatePty::default()
    };
    let first = conn.create_pty(params.clone()).await.unwrap().unwrap();
    let second = conn.create_pty(params).await.unwrap().unwrap();
    assert_eq!(first, second);
    conn.kill_pty(&first).await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn attaching_to_an_unknown_pty_ends_the_stream() {
    let daemon = TestDaemon::start();
    let conn = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();
    let mut attachment = conn.attach_pty("no-such-pty", AttachKind::Background);
    let next = tokio::time::timeout(Duration::from_secs(5), attachment.events.next())
        .await
        .expect("stream should end promptly");
    assert!(next.is_none(), "expected the stream to end, got {next:?}");

    // Writes then fail with why, as Electron's pending writes do.
    let ended = Err(PtyClosed(STREAM_ENDED.to_owned()));
    assert_eq!(attachment.input.write(b"x").await, ended);
    assert_eq!(attachment.input.send(b"x"), ended);
    assert_eq!(attachment.input.resize(100, 30), ended);
    assert_eq!(attachment.input.focus(), ended);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_after_a_detach_fail_with_pty_stream_detached() {
    let daemon = TestDaemon::start();
    let conn = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();
    let pty = spawn_shell(&conn, &daemon).await;
    let mut attachment = conn.attach_pty(&pty, AttachKind::Terminal);
    read_until(&mut attachment, |seen| seen.contains(&PtyEvent::ReplayEnd)).await;

    let PtyAttachment { input, events } = attachment;
    drop(events);
    let closed = tokio::time::timeout(Duration::from_secs(5), input.closed())
        .await
        .expect("the detach should be recorded promptly");
    assert_eq!(closed, PtyClosed(STREAM_DETACHED.to_owned()));
    assert_eq!(input.write(b"x").await, Err(closed));
    conn.kill_pty(&pty).await.unwrap().unwrap();
}

/// A test's runtime ends right after the test drops its daemon, so the daemon may be
/// cancelled before its shutdown ran. Its shells are hung up all the same: the runtime's drop
/// waits for each shell's waitpid thread, which never returned while a cancelled daemon left
/// the shell's terminal open (the connection tests hung at exit).
#[test]
fn a_daemon_cancelled_with_its_runtime_hangs_up_its_shells() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let daemon = runtime.block_on(async {
        let daemon = TestDaemon::start();
        let conn = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();
        let pty = spawn_shell(&conn, &daemon).await;
        // Idle at its prompt, as the connection test's shell is when it ends (zsh can
        // deadlock in its own hangup handler when the hangup lands in a prompt hook).
        let mut attachment = conn.attach_pty(&pty, AttachKind::Background);
        let idle = tokio::time::timeout(Duration::from_secs(10), async {
            let mut printed = false;
            loop {
                match tokio::time::timeout(Duration::from_millis(300), attachment.events.next()).await {
                    Ok(Some(event)) => {
                        printed |= matches!(event.expect("pty stream error"), PtyEvent::Data(_));
                    }
                    Ok(None) => panic!("pty stream ended"),
                    Err(_quiet) if printed => return,
                    Err(_quiet) => {}
                }
            }
        })
        .await;
        assert!(idle.is_ok(), "the shell never went quiet at its prompt");
        daemon
    });

    // The daemon is still serving: the runtime cancels it, it is never told to stop.
    let started = std::time::Instant::now();
    runtime.shutdown_timeout(Duration::from_secs(30));
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the runtime waited for a shell that was never hung up"
    );
    drop(daemon);
}
