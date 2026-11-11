//! Filesystem calls against a real in-process daemon.

use std::time::Duration;

use futures::StreamExt;
use treeterm_core::daemon::ApiError;
use treeterm_core::fs::{
    FileSignal, FsClient, FsError, ReadOutcome, WriteGuard, WriteOutcome, sha256_hex,
};
use treeterm_core::rt::{Rt, Sub};
use treeterm_core::transport::{connect_unix, daemon_client};
use treeterm_testkit::TestDaemon;

fn rt() -> Rt {
    Rt::new(tokio::runtime::Handle::current())
}

async fn client(daemon: &TestDaemon) -> FsClient {
    let channel = connect_unix(daemon.socket_path()).await.unwrap();
    FsClient::new(rt(), daemon_client(channel))
}

/// The scope every call works in.
fn scope(daemon: &TestDaemon) -> String {
    daemon.root().to_string_lossy().into_owned()
}

async fn write(
    fs: &FsClient,
    scope: &str,
    path: &str,
    content: &[u8],
    guard: WriteGuard,
) -> WriteOutcome {
    fs.write_file(scope, path, content.to_vec(), guard)
        .await
        .unwrap()
        .unwrap()
}

async fn read(fs: &FsClient, scope: &str, path: &str) -> Result<ReadOutcome, FsError> {
    fs.read_file(scope, path, 10 * 1024 * 1024).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_then_read_round_trips_with_the_written_hash() {
    let daemon = TestDaemon::start();
    let fs = client(&daemon).await;
    let scope = scope(&daemon);

    let written = write(
        &fs,
        &scope,
        "notes/a.rs",
        b"hello",
        WriteGuard::Unconditional,
    )
    .await;
    assert_eq!(
        written,
        WriteOutcome::Written {
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".to_owned()
        }
    );
    assert_eq!(
        std::fs::read(daemon.root().join("notes/a.rs")).unwrap(),
        b"hello"
    );

    let ReadOutcome::Read(file) = read(&fs, &scope, "notes/a.rs").await.unwrap() else {
        panic!("expected the file to be read");
    };
    assert_eq!(file.content, b"hello");
    assert_eq!(file.header.size, 5);
    assert_eq!(file.header.language, "rust");
    assert!(
        file.header.path.ends_with("notes/a.rs"),
        "{}",
        file.header.path
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_large_file_is_written_and_read_in_chunks() {
    let daemon = TestDaemon::start();
    let fs = client(&daemon).await;
    let scope = scope(&daemon);
    let content: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();

    let written = write(&fs, &scope, "big.bin", &content, WriteGuard::MustNotExist).await;
    assert_eq!(
        written,
        WriteOutcome::Written {
            sha256: sha256_hex(&content)
        }
    );
    let ReadOutcome::Read(file) = read(&fs, &scope, "big.bin").await.unwrap() else {
        panic!("expected the file to be read");
    };
    assert_eq!(file.content, content);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compare_and_swap_rejects_stale_writes() {
    let daemon = TestDaemon::start();
    let fs = client(&daemon).await;
    let scope = scope(&daemon);

    let WriteOutcome::Written { sha256: first } =
        write(&fs, &scope, "state.json", b"{}", WriteGuard::MustNotExist).await
    else {
        panic!("a new file must be writable with MustNotExist");
    };
    assert_eq!(
        write(
            &fs,
            &scope,
            "state.json",
            b"{\"a\":1}",
            WriteGuard::MustNotExist
        )
        .await,
        WriteOutcome::Conflict
    );

    let WriteOutcome::Written { sha256: second } = write(
        &fs,
        &scope,
        "state.json",
        b"{\"a\":1}",
        WriteGuard::Matches(first.clone()),
    )
    .await
    else {
        panic!("the current hash must allow the write");
    };
    assert_eq!(
        write(
            &fs,
            &scope,
            "state.json",
            b"{\"a\":2}",
            WriteGuard::Matches(first)
        )
        .await,
        WriteOutcome::Conflict,
        "a stale hash must not overwrite"
    );
    assert_eq!(
        std::fs::read(daemon.root().join("state.json")).unwrap(),
        b"{\"a\":1}"
    );
    assert_eq!(second, sha256_hex(b"{\"a\":1}"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_over_budget_is_refused_on_its_header() {
    let daemon = TestDaemon::start();
    let fs = client(&daemon).await;
    let scope = scope(&daemon);
    std::fs::write(daemon.root().join("five.txt"), b"12345").unwrap();

    let outcome = fs.read_file(&scope, "five.txt", 4).await.unwrap().unwrap();
    assert_eq!(outcome, ReadOutcome::TooLarge { size: 5 });
    let fits = fs.read_file(&scope, "five.txt", 5).await.unwrap().unwrap();
    assert!(matches!(fits, ReadOutcome::Read(_)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reading_a_missing_file_reports_the_daemons_error() {
    let daemon = TestDaemon::start();
    let fs = client(&daemon).await;
    let result = read(&fs, &scope(&daemon), "missing.txt").await;
    assert!(
        matches!(&result, Err(FsError::Daemon(message)) if message.contains("No such file")),
        "{result:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paths_outside_the_workspace_are_refused() {
    let daemon = TestDaemon::start();
    let fs = client(&daemon).await;
    let scope = daemon.workspaces_dir().to_string_lossy().into_owned();
    std::fs::write(daemon.root().join("outside.txt"), b"secret").unwrap();

    let read = read(&fs, &scope, "../outside.txt").await;
    assert!(
        matches!(&read, Err(FsError::Daemon(message)) if message.contains("Access denied")),
        "{read:?}"
    );
    let write = fs
        .write_file(
            &scope,
            "../escape.txt",
            b"x".to_vec(),
            WriteGuard::Unconditional,
        )
        .await
        .unwrap();
    assert!(
        matches!(&write, Err(FsError::Daemon(message)) if message.contains("Access denied")),
        "{write:?}"
    );
    assert!(!daemon.root().join("escape.txt").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_is_idempotent() {
    let daemon = TestDaemon::start();
    let fs = client(&daemon).await;
    let scope = scope(&daemon);
    write(&fs, &scope, "gone.txt", b"bye", WriteGuard::Unconditional).await;

    fs.delete_file(&scope, "gone.txt").await.unwrap().unwrap();
    assert!(!daemon.root().join("gone.txt").exists());
    fs.delete_file(&scope, "gone.txt").await.unwrap().unwrap();
    assert!(read(&fs, &scope, "gone.txt").await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_directory_lists_directories_first_and_skips_hidden_entries() {
    let daemon = TestDaemon::start();
    let fs = client(&daemon).await;
    let scope = scope(&daemon);
    let dir = daemon.root().join("tree");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("b.txt"), b"b").unwrap();
    std::fs::write(dir.join("a.txt"), b"aa").unwrap();
    std::fs::write(dir.join(".hidden"), b"h").unwrap();

    let contents = fs.read_directory(&scope, "tree").await.unwrap().unwrap();
    let names: Vec<(&str, bool)> = contents
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.is_directory))
        .collect();
    assert_eq!(
        names,
        vec![("sub", true), ("a.txt", false), ("b.txt", false)]
    );
    let a = &contents.entries[1];
    assert_eq!(a.relative_path, "tree/a.txt");
    assert_eq!(a.size, Some(2));

    let missing = fs.read_directory(&scope, "no-such-dir").await.unwrap();
    assert!(matches!(missing, Err(FsError::Daemon(_))), "{missing:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_files_matches_names_case_insensitively() {
    let daemon = TestDaemon::start();
    let fs = client(&daemon).await;
    let scope = scope(&daemon);
    std::fs::create_dir_all(daemon.root().join("src/node_modules")).unwrap();
    std::fs::write(daemon.root().join("src/GitClient.rs"), b"").unwrap();
    std::fs::write(daemon.root().join("src/other.rs"), b"").unwrap();
    std::fs::write(daemon.root().join("src/node_modules/gitclient.js"), b"").unwrap();

    let entries = fs.search_files(&scope, "gitclient").await.unwrap().unwrap();
    let found: Vec<&str> = entries
        .iter()
        .map(|entry| entry.relative_path.as_str())
        .collect();
    assert_eq!(found, vec!["src/GitClient.rs"]);
    assert!(
        fs.search_files(&scope, "  ")
            .await
            .unwrap()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watch_file_signal_reports_the_hash_then_absence() {
    let daemon = TestDaemon::start();
    let fs = client(&daemon).await;
    let scope = scope(&daemon);
    let WriteOutcome::Written { sha256 } = write(
        &fs,
        &scope,
        "watched.json",
        b"{}",
        WriteGuard::Unconditional,
    )
    .await
    else {
        panic!("write failed");
    };

    let mut signals = fs.watch_file_signal(&scope, "watched.json");
    assert_eq!(
        next_signal(&mut signals).await,
        FileSignal::Present { sha256 }
    );
    fs.delete_file(&scope, "watched.json")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next_signal(&mut signals).await, FileSignal::Absent);
}

async fn next_signal(signals: &mut Sub<Result<FileSignal, ApiError>>) -> FileSignal {
    tokio::time::timeout(Duration::from_secs(10), signals.next())
        .await
        .expect("timed out")
        .expect("stream ended")
        .expect("watch failed")
}
