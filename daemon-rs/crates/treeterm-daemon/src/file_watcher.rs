use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{EventKind, Watcher};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tonic::Status;
use treeterm_proto::treeterm::*;

use crate::filesystem;

/// Last observed file state, the dedupe key shared by all three change-detection
/// mechanisms (OS-native notify, polling re-read, WriteFile/DeleteFile intercept).
#[derive(Clone, PartialEq)]
enum LastState {
    Absent,
    Present { sha256: String },
}

/// A point-in-time read of the file: state plus the bytes needed to build events.
enum Snapshot {
    Absent,
    Present { sha256: String, content: Vec<u8> },
}

impl Snapshot {
    fn last_state(&self) -> LastState {
        match self {
            Snapshot::Absent => LastState::Absent,
            Snapshot::Present { sha256, .. } => LastState::Present { sha256: sha256.clone() },
        }
    }

    fn content_event(&self) -> FileWatchEvent {
        FileWatchEvent {
            state: Some(match self {
                Snapshot::Absent => file_watch_event::State::Absent(FileAbsent {}),
                Snapshot::Present { sha256, content } => file_watch_event::State::Present(FilePresent {
                    content: content.clone(),
                    sha256: sha256.clone(),
                }),
            }),
        }
    }

    fn signal_event(&self) -> FileSignalEvent {
        FileSignalEvent {
            state: Some(match self {
                Snapshot::Absent => file_signal_event::State::Absent(FileAbsent {}),
                Snapshot::Present { sha256, .. } => {
                    file_signal_event::State::Present(FileSignalPresent { sha256: sha256.clone() })
                }
            }),
        }
    }
}

enum EventTx {
    Content(mpsc::Sender<Result<FileWatchEvent, Status>>),
    Signal(mpsc::Sender<Result<FileSignalEvent, Status>>),
}

impl EventTx {
    /// try_send with the session-watch retention policy: keep the subscriber on Full
    /// (events are full-state, the next delivery self-heals), report Closed for removal.
    fn send_snapshot(&self, snapshot: &Snapshot) -> bool {
        match self {
            EventTx::Content(tx) => !matches!(
                tx.try_send(Ok(snapshot.content_event())),
                Err(mpsc::error::TrySendError::Closed(_))
            ),
            EventTx::Signal(tx) => !matches!(
                tx.try_send(Ok(snapshot.signal_event())),
                Err(mpsc::error::TrySendError::Closed(_))
            ),
        }
    }

    fn send_error(&self, status: Status) {
        match self {
            EventTx::Content(tx) => { let _ = tx.try_send(Err(status)); }
            EventTx::Signal(tx) => { let _ = tx.try_send(Err(status)); }
        }
    }

    fn is_closed(&self) -> bool {
        match self {
            EventTx::Content(tx) => tx.is_closed(),
            EventTx::Signal(tx) => tx.is_closed(),
        }
    }
}

struct Subscriber {
    tx: EventTx,
}

struct WatchEntry {
    last_state: LastState,
    subscribers: Vec<Subscriber>,
    /// Whether the OS-native watch on the parent dir is active. When false
    /// (e.g. NFS), polling carries the entry and retries registration each pass.
    notify_registered: bool,
}

struct Inner {
    entries: HashMap<PathBuf, WatchEntry>,
    notify_watcher: Option<notify::RecommendedWatcher>,
}

/// Watches files for change, notifying gRPC subscribers. Three concurrent
/// mechanisms feed it — OS-native notify events, a polling re-read loop, and
/// direct interception of the daemon's own WriteFile/DeleteFile RPCs — all
/// deduped through the entry's last observed content SHA-256.
#[derive(Clone)]
pub struct FileWatcher {
    inner: Arc<Mutex<Inner>>,
}

/// Whether an OS-native event could reflect a change to file content or existence.
///
/// The parent-directory watch registered by notify includes `IN_OPEN`, so every
/// `open()` on a watched file — including the ones `reconcile` itself performs to
/// re-read the file — surfaces here. Forwarding those `Access` events would make
/// each reconcile-read trigger another reconcile, a self-sustaining CPU storm that
/// never terminates because the directory content never actually changes. Real
/// changes always arrive as `Modify`/`Create`/`Remove`, so `Access` is dropped.
/// `Any`/`Other` are kept: they are the conservative "something happened" fallbacks.
fn event_may_change_content(kind: &EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

/// Stable key for a watched file: canonicalized parent (when it exists) + file name,
/// so subscribe-time keys and intercept-time keys always match.
fn normalize(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .map(|p| p.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

async fn read_snapshot(path: &Path) -> Result<Snapshot, Status> {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            if bytes.len() as u64 > filesystem::MAX_FILE_SIZE {
                return Err(Status::failed_precondition("File too large to watch (max 64KB)"));
            }
            let sha256 = filesystem::sha256_hex(&bytes);
            Ok(Snapshot::Present { sha256, content: bytes })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Snapshot::Absent),
        Err(e) => Err(Status::internal(format!("Failed to read watched file: {e}"))),
    }
}

impl FileWatcher {
    pub fn new(poll_interval: Duration) -> Self {
        let (notify_tx, mut notify_rx) = mpsc::unbounded_channel::<PathBuf>();
        let notify_watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if !event_may_change_content(&event.kind) {
                    return;
                }
                for path in event.paths {
                    let _ = notify_tx.send(path);
                }
            }
        })
        .map_err(|e| tracing::warn!(error = %e, "OS file watcher unavailable, relying on polling"))
        .ok();

        let watcher = Self {
            inner: Arc::new(Mutex::new(Inner {
                entries: HashMap::new(),
                notify_watcher,
            })),
        };

        // Mechanism A: OS-native events → reconcile the touched path.
        let for_notify = watcher.clone();
        tokio::spawn(async move {
            while let Some(path) = notify_rx.recv().await {
                for_notify.reconcile(&normalize(&path)).await;
            }
        });

        // Mechanism B: polling loop — reconcile every entry, GC dead subscribers,
        // retry failed notify registrations. This is what carries NFS mounts.
        let for_poll = watcher.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(poll_interval).await;
                for_poll.poll_pass().await;
            }
        });

        watcher
    }

    pub async fn subscribe_content(
        &self,
        workspace_path: &Path,
        file_path: &str,
        watcher_id: String,
        tx: mpsc::Sender<Result<FileWatchEvent, Status>>,
    ) -> Result<(), Status> {
        self.subscribe(workspace_path, file_path, watcher_id, EventTx::Content(tx)).await
    }

    pub async fn subscribe_signal(
        &self,
        workspace_path: &Path,
        file_path: &str,
        watcher_id: String,
        tx: mpsc::Sender<Result<FileSignalEvent, Status>>,
    ) -> Result<(), Status> {
        self.subscribe(workspace_path, file_path, watcher_id, EventTx::Signal(tx)).await
    }

    async fn subscribe(
        &self,
        workspace_path: &Path,
        file_path: &str,
        watcher_id: String,
        tx: EventTx,
    ) -> Result<(), Status> {
        let resolved = filesystem::resolve_target(workspace_path, file_path);
        if !filesystem::is_path_within_workspace(workspace_path, &resolved).await {
            return Err(Status::permission_denied("Access denied: Path outside workspace"));
        }
        tracing::debug!(watcher_id = %watcher_id, path = %resolved.display(), "file watch subscribed");
        let key = normalize(&resolved);

        // Snapshot and registration happen under one lock so the initial event and
        // subsequent broadcasts can never miss or reorder an update in between.
        let mut inner = self.inner.lock().await;
        let snapshot = read_snapshot(&key).await?;

        let entry = inner.entries.entry(key.clone()).or_insert_with(|| WatchEntry {
            last_state: snapshot.last_state(),
            subscribers: Vec::new(),
            notify_registered: false,
        });

        // The read doubles as a reconcile: existing subscribers learn about any
        // change that happened since the last mechanism fired.
        if entry.last_state != snapshot.last_state() {
            entry.last_state = snapshot.last_state();
            entry.subscribers.retain(|s| s.tx.send_snapshot(&snapshot));
        }

        // Fresh channel: the initial event always fits.
        tx.send_snapshot(&snapshot);
        entry.subscribers.push(Subscriber { tx });

        Self::register_notify(&mut inner, &key);
        Ok(())
    }

    /// Mechanism C: a successful WriteFile RPC through this daemon. Broadcasts the
    /// written content directly — instant even where inotify can't see the change.
    pub async fn notify_written(&self, resolved: &Path, content: &[u8]) {
        let snapshot = Snapshot::Present {
            sha256: filesystem::sha256_hex(content),
            content: content.to_vec(),
        };
        self.apply_snapshot(&normalize(resolved), snapshot).await;
    }

    /// Mechanism C for DeleteFile.
    pub async fn notify_deleted(&self, resolved: &Path) {
        self.apply_snapshot(&normalize(resolved), Snapshot::Absent).await;
    }

    /// Re-read a watched path and broadcast if its state changed.
    async fn reconcile(&self, key: &Path) {
        let mut inner = self.inner.lock().await;
        let Some(entry) = inner.entries.get_mut(key) else { return };
        match read_snapshot(key).await {
            Ok(snapshot) => {
                if entry.last_state != snapshot.last_state() {
                    entry.last_state = snapshot.last_state();
                    entry.subscribers.retain(|s| s.tx.send_snapshot(&snapshot));
                }
            }
            Err(status) => {
                // Fail loudly: subscribers see the stream error and can re-subscribe.
                tracing::warn!(path = %key.display(), error = %status, "watched file became unreadable");
                for sub in entry.subscribers.drain(..) {
                    sub.tx.send_error(status.clone());
                }
            }
        }
    }

    async fn apply_snapshot(&self, key: &Path, snapshot: Snapshot) {
        let mut inner = self.inner.lock().await;
        let Some(entry) = inner.entries.get_mut(key) else { return };
        if entry.last_state != snapshot.last_state() {
            entry.last_state = snapshot.last_state();
            entry.subscribers.retain(|s| s.tx.send_snapshot(&snapshot));
        }
    }

    async fn poll_pass(&self) {
        let keys: Vec<PathBuf> = self.inner.lock().await.entries.keys().cloned().collect();
        for key in keys {
            self.reconcile(&key).await;
        }

        let mut inner = self.inner.lock().await;
        let registered_parents = |inner: &Inner| -> Vec<PathBuf> {
            inner
                .entries
                .iter()
                .filter(|(_, e)| e.notify_registered)
                .filter_map(|(k, _)| k.parent().map(Path::to_path_buf))
                .collect()
        };
        let parents_before = registered_parents(&inner);

        inner.entries.retain(|_, entry| {
            entry.subscribers.retain(|s| !s.tx.is_closed());
            !entry.subscribers.is_empty()
        });

        // Unwatch parent dirs that no longer back any registered entry,
        // then retry registration for entries still missing their OS watch.
        let parents_after = registered_parents(&inner);
        if let Some(w) = inner.notify_watcher.as_mut() {
            for parent in parents_before {
                if !parents_after.contains(&parent) {
                    let _ = w.unwatch(&parent);
                }
            }
        }
        let keys: Vec<PathBuf> = inner.entries.keys().cloned().collect();
        for key in keys {
            Self::register_notify(&mut inner, &key);
        }
    }

    /// Idempotently register the OS-native watch on the entry's parent directory.
    /// Failure (NFS, missing parent) is non-fatal: polling covers the entry and this
    /// is retried every poll pass.
    fn register_notify(inner: &mut Inner, key: &Path) {
        let Some(entry) = inner.entries.get(key) else { return };
        if entry.notify_registered {
            return;
        }
        let Some(parent) = key.parent() else { return };
        let Some(watcher) = inner.notify_watcher.as_mut() else { return };
        match watcher.watch(parent, notify::RecursiveMode::NonRecursive) {
            Ok(()) => {
                if let Some(entry) = inner.entries.get_mut(key) {
                    entry.notify_registered = true;
                }
            }
            Err(e) => {
                tracing::warn!(path = %key.display(), error = %e, "OS watch registration failed, polling covers this file");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn channel_content() -> (mpsc::Sender<Result<FileWatchEvent, Status>>, mpsc::Receiver<Result<FileWatchEvent, Status>>) {
        mpsc::channel(16)
    }

    fn channel_signal() -> (mpsc::Sender<Result<FileSignalEvent, Status>>, mpsc::Receiver<Result<FileSignalEvent, Status>>) {
        mpsc::channel(16)
    }

    fn present(event: FileWatchEvent) -> FilePresent {
        match event.state.unwrap() {
            file_watch_event::State::Present(p) => p,
            file_watch_event::State::Absent(_) => panic!("expected Present"),
        }
    }

    fn assert_absent(event: FileWatchEvent) {
        assert!(matches!(event.state.unwrap(), file_watch_event::State::Absent(_)));
    }

    /// Long poll interval: tests below exercise the intercept path deterministically.
    fn watcher() -> FileWatcher {
        FileWatcher::new(Duration::from_secs(3600))
    }

    #[test]
    fn access_events_are_ignored_but_changes_are_forwarded() {
        use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind, RemoveKind};

        // Access (open/close/read) must be dropped: reconcile's own reads produce
        // these, and forwarding them re-triggers reconcile in an endless loop.
        assert!(!event_may_change_content(&EventKind::Access(AccessKind::Open(AccessMode::Read))));
        assert!(!event_may_change_content(&EventKind::Access(AccessKind::Close(AccessMode::Write))));
        assert!(!event_may_change_content(&EventKind::Access(AccessKind::Any)));

        // Real content/existence changes must always be forwarded.
        assert!(event_may_change_content(&EventKind::Modify(ModifyKind::Any)));
        assert!(event_may_change_content(&EventKind::Create(CreateKind::File)));
        assert!(event_may_change_content(&EventKind::Remove(RemoveKind::File)));
        assert!(event_may_change_content(&EventKind::Any));
    }

    #[tokio::test]
    async fn initial_event_is_absent_for_missing_file() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let fw = watcher();
        let (tx, mut rx) = channel_content();

        fw.subscribe_content(&ws, "missing.json", "w1".into(), tx).await.unwrap();

        assert_absent(rx.try_recv().unwrap().unwrap());
    }

    #[tokio::test]
    async fn initial_event_is_present_with_sha_for_existing_file() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        tokio::fs::write(ws.join("a.json"), b"hello").await.unwrap();
        let fw = watcher();
        let (tx, mut rx) = channel_content();

        fw.subscribe_content(&ws, "a.json", "w1".into(), tx).await.unwrap();

        let p = present(rx.try_recv().unwrap().unwrap());
        assert_eq!(p.content, b"hello");
        assert_eq!(p.sha256, filesystem::sha256_hex(b"hello"));
    }

    #[tokio::test]
    async fn subscribe_rejects_path_outside_workspace() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let fw = watcher();
        let (tx, _rx) = channel_content();

        let err = fw
            .subscribe_content(&ws, "../escape.json", "w1".into(), tx)
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn subscribe_rejects_oversize_file() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let big = vec![0u8; (filesystem::MAX_FILE_SIZE + 1) as usize];
        tokio::fs::write(ws.join("big.bin"), &big).await.unwrap();
        let fw = watcher();
        let (tx, _rx) = channel_content();

        let err = fw
            .subscribe_content(&ws, "big.bin", "w1".into(), tx)
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn notify_written_broadcasts_to_content_and_signal_subscribers() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let fw = watcher();
        let (ctx, mut crx) = channel_content();
        let (stx, mut srx) = channel_signal();

        fw.subscribe_content(&ws, "a.json", "c".into(), ctx).await.unwrap();
        fw.subscribe_signal(&ws, "a.json", "s".into(), stx).await.unwrap();
        crx.try_recv().unwrap().unwrap(); // initial absent
        srx.try_recv().unwrap().unwrap();

        fw.notify_written(&ws.join("a.json"), b"v1").await;

        let p = present(crx.try_recv().unwrap().unwrap());
        assert_eq!(p.content, b"v1");
        let s = srx.try_recv().unwrap().unwrap();
        match s.state.unwrap() {
            file_signal_event::State::Present(sp) => assert_eq!(sp.sha256, filesystem::sha256_hex(b"v1")),
            file_signal_event::State::Absent(_) => panic!("expected Present"),
        }
    }

    #[tokio::test]
    async fn notify_written_with_same_content_emits_nothing() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        tokio::fs::write(ws.join("a.json"), b"same").await.unwrap();
        let fw = watcher();
        let (tx, mut rx) = channel_content();

        fw.subscribe_content(&ws, "a.json", "w".into(), tx).await.unwrap();
        rx.try_recv().unwrap().unwrap(); // initial present

        fw.notify_written(&ws.join("a.json"), b"same").await;

        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn notify_deleted_broadcasts_absent() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        tokio::fs::write(ws.join("a.json"), b"v1").await.unwrap();
        let fw = watcher();
        let (tx, mut rx) = channel_content();

        fw.subscribe_content(&ws, "a.json", "w".into(), tx).await.unwrap();
        rx.try_recv().unwrap().unwrap(); // initial present

        fw.notify_deleted(&ws.join("a.json")).await;

        assert_absent(rx.try_recv().unwrap().unwrap());

        // Absent → Present again completes the round trip.
        fw.notify_written(&ws.join("a.json"), b"v2").await;
        let p = present(rx.try_recv().unwrap().unwrap());
        assert_eq!(p.content, b"v2");
    }

    #[tokio::test]
    async fn notify_for_unwatched_path_is_ignored() {
        let fw = watcher();
        // No subscribers at all — must not panic or create entries.
        fw.notify_written(Path::new("/tmp/nowhere.json"), b"x").await;
        fw.notify_deleted(Path::new("/tmp/nowhere.json")).await;
        assert!(fw.inner.lock().await.entries.is_empty());
    }

    #[tokio::test]
    async fn polling_detects_out_of_band_write() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let fw = FileWatcher::new(Duration::from_millis(20));
        let (tx, mut rx) = channel_content();

        fw.subscribe_content(&ws, "a.json", "w".into(), tx).await.unwrap();
        assert_absent(rx.try_recv().unwrap().unwrap());

        // Simulates another VM writing over NFS: no RPC intercept, no local inotify
        // guarantee — only the poll loop (or notify, when supported) may catch it.
        tokio::fs::write(ws.join("a.json"), b"external").await.unwrap();

        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("poll loop should detect the change")
            .unwrap()
            .unwrap();
        assert_eq!(present(event).content, b"external");
    }

    #[tokio::test]
    async fn subscribe_reconciles_existing_subscribers_on_stale_state() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let fw = watcher();
        let (tx1, mut rx1) = channel_content();
        fw.subscribe_content(&ws, "a.json", "w1".into(), tx1).await.unwrap();
        assert_absent(rx1.try_recv().unwrap().unwrap());

        // Out-of-band write that no mechanism has observed yet.
        tokio::fs::write(ws.join("a.json"), b"sneaky").await.unwrap();

        // A second subscriber's snapshot doubles as a reconcile for the first.
        let (tx2, mut rx2) = channel_content();
        fw.subscribe_content(&ws, "a.json", "w2".into(), tx2).await.unwrap();
        assert_eq!(present(rx2.try_recv().unwrap().unwrap()).content, b"sneaky");
        assert_eq!(present(rx1.try_recv().unwrap().unwrap()).content, b"sneaky");
    }

    #[tokio::test]
    async fn poll_pass_prunes_closed_subscribers_and_empty_entries() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let fw = watcher();
        let (tx, rx) = channel_content();

        fw.subscribe_content(&ws, "a.json", "w".into(), tx).await.unwrap();
        drop(rx);

        fw.poll_pass().await;

        assert!(fw.inner.lock().await.entries.is_empty());
    }

    #[tokio::test]
    async fn two_subscribers_share_one_entry() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let fw = watcher();
        let (tx1, _rx1) = channel_content();
        let (tx2, _rx2) = channel_signal();

        fw.subscribe_content(&ws, "a.json", "w1".into(), tx1).await.unwrap();
        fw.subscribe_signal(&ws, "a.json", "w2".into(), tx2).await.unwrap();

        let inner = fw.inner.lock().await;
        assert_eq!(inner.entries.len(), 1);
        assert_eq!(inner.entries.values().next().unwrap().subscribers.len(), 2);
    }
}
