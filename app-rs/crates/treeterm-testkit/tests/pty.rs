//! PTY attachments against a real in-process daemon.

use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt;
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::pty::{AttachKind, CreatePty, PtyAttachment, PtyEvent};
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attaching_to_an_unknown_pty_ends_the_stream() {
    let daemon = TestDaemon::start();
    let conn = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();
    let mut attachment = conn.attach_pty("no-such-pty", AttachKind::Background);
    let next = tokio::time::timeout(Duration::from_secs(5), attachment.events.next())
        .await
        .expect("stream should end promptly");
    assert!(next.is_none(), "expected the stream to end, got {next:?}");
}
