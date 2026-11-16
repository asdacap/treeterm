//! `settings.json` as an entity: the current settings, saved on change, and reloaded when
//! another writer (the Electron app, or a hand edit) changes the file.
//!
//! Port of the renderer's settings store plus main's load/save. The file is shared with the
//! Electron app, so it is polled for changes (by modification time) rather than owned.
//!
//! Saves are written in the background, one at a time in the order they were asked for, and
//! reach [`SettingsStore::settings`] only once written: a failed save changes nothing but the
//! error (Electron's `saveSettings` and `toggleSound` set the settings only on success). A
//! `settings.json` that could not be read is left as it is until an explicit save
//! ([`SettingsStore::save`]): changes made in passing ([`SettingsStore::update`]) are dropped.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use futures::channel::oneshot;
use gpui_kit::{Context, EventEmitter, Task};
use treeterm_core::settings::{self, Settings, SettingsError, SshConnectionConfig};

/// How often the file is checked for edits made elsewhere.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsEvent {
    Changed,
}

impl EventEmitter<SettingsEvent> for SettingsStore {}

/// The sound toggle's own save (`SoundSaveState`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundSave {
    Idle,
    /// The toggle's write is running; further toggles are ignored.
    Saving,
    /// The last toggle was not saved, and why; shown until the next toggle.
    Failed(String),
}

/// Told a save's outcome.
type SaveReply = oneshot::Sender<Result<(), String>>;

/// Who asked for a save, which decides whether it may replace a `settings.json` that could
/// not be read.
enum SaveKind {
    /// `saveSettings` (Save in Settings, the sound toggle): written whatever the file holds,
    /// and told the write's outcome.
    Explicit(SaveReply),
    /// A change made in passing, not awaited (a folder picked with Browse..., a saved or
    /// removed SSH connection, port forwards, the prompt debugger's Update): never written
    /// over a file that could not be read.
    InPassing,
}

impl SaveKind {
    fn reply(self, outcome: Result<(), String>) {
        match self {
            SaveKind::Explicit(done) => match done.send(outcome) {
                Ok(()) | Err(_) => {} // Err: the caller stopped waiting
            },
            SaveKind::InPassing => {}
        }
    }
}

/// A change waiting to be written, applied to the settings as they are when its turn comes.
struct QueuedSave {
    change: Box<dyn FnOnce(&mut Settings)>,
    kind: SaveKind,
}

/// What `settings.json` holds, as far as the store knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnDisk {
    /// The settings last read or written, or no file at all.
    Known,
    /// Something that could not be read (a hand edit gone wrong, settings Electron's schema
    /// rejects). The settings held in its place (the defaults, or the last read) are not
    /// what it holds, so only an explicit save replaces it.
    Unreadable,
}

pub struct SettingsStore {
    /// `None` when there is no user data dir (settings stay in memory).
    user_data_dir: Option<PathBuf>,
    settings: Rc<Settings>,
    /// The file's modification time when last read or written by us.
    seen: Option<SystemTime>,
    /// The last load or save failure.
    error: Option<String>,
    on_disk: OnDisk,
    sound: SoundSave,
    queue: VecDeque<QueuedSave>,
    /// Writes `queue` front to back; `Some` while there is anything to write.
    writer: Option<Task<()>>,
    /// Waits for the sound toggle's save.
    _sound_save: Option<Task<()>>,
    _poll: Option<Task<()>>,
}

fn modified(dir: &Path) -> Option<SystemTime> {
    std::fs::metadata(settings::settings_path(dir)).and_then(|m| m.modified()).ok()
}

impl SettingsStore {
    /// Load from `user_data_dir` and start watching. A missing file is the defaults, written
    /// to disk as Electron's `loadSettings` does; an unreadable or corrupt one keeps the
    /// defaults (and the file, until an explicit save) and records the error.
    pub fn new(user_data_dir: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let mut store = Self {
            user_data_dir,
            settings: Rc::new(Settings::default()),
            seen: None,
            error: None,
            on_disk: OnDisk::Known,
            sound: SoundSave::Idle,
            queue: VecDeque::new(),
            writer: None,
            _sound_save: None,
            _poll: None,
        };
        store.reload();
        store.create_file();
        if store.user_data_dir.is_some() {
            let executor = cx.background_executor().clone();
            store._poll = Some(cx.spawn(async move |this, cx| {
                loop {
                    executor.timer(POLL_INTERVAL).await;
                    let Ok(()) = this.update(cx, |store, cx| store.check_for_edits(cx)) else {
                        return;
                    };
                }
            }));
        }
        store
    }

    /// In-memory settings, for tests and for running without a user data dir.
    pub fn in_memory(settings: Settings) -> Self {
        Self {
            user_data_dir: None,
            settings: Rc::new(settings),
            seen: None,
            error: None,
            on_disk: OnDisk::Known,
            sound: SoundSave::Idle,
            queue: VecDeque::new(),
            writer: None,
            _sound_save: None,
            _poll: None,
        }
    }

    pub fn settings(&self) -> &Rc<Settings> {
        &self.settings
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn sound_save(&self) -> &SoundSave {
        &self.sound
    }

    fn reload(&mut self) -> bool {
        let Some(dir) = &self.user_data_dir else {
            return false;
        };
        self.seen = modified(dir);
        match settings::load(dir) {
            Ok(loaded) => {
                self.error = None;
                self.on_disk = OnDisk::Known;
                if *self.settings == loaded {
                    return false;
                }
                self.settings = Rc::new(loaded);
                true
            }
            Err(error) => {
                tracing::error!(%error, "settings unreadable; keeping the current settings");
                self.error = Some(error.to_string());
                self.on_disk = OnDisk::Unreadable;
                false
            }
        }
    }

    /// Write the settings when there is no file yet, as Electron's `loadSettings` does.
    fn create_file(&mut self) {
        let Some(dir) = &self.user_data_dir else {
            return;
        };
        match settings::create_if_missing(dir, &self.settings) {
            Ok(true) => self.seen = modified(dir),
            Ok(false) => {}
            Err(error) => {
                tracing::error!(%error, "cannot create the settings file");
                self.error = Some(error.to_string());
            }
        }
    }

    fn check_for_edits(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = &self.user_data_dir else {
            return;
        };
        // Our own write is landing: it would be read half-written, and `seen` moves past it.
        if self.writer.is_some() {
            return;
        }
        if modified(dir) == self.seen {
            return;
        }
        if self.reload() {
            cx.emit(SettingsEvent::Changed);
        }
        cx.notify();
    }

    /// Change settings and persist them, without waiting: see [`Self::save`]. A failed save
    /// is logged and recorded in [`Self::error`]. While `settings.json` holds something that
    /// could not be read, the change is dropped (and logged) when its turn comes, so the
    /// settings held in its place never overwrite it in passing: only [`Self::save`] does.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings) + 'static, cx: &mut Context<Self>) {
        self.enqueue(Box::new(change), SaveKind::InPassing, cx);
    }

    /// Change settings and persist them (`saveSettings`). The change is applied to the
    /// settings as they are once every earlier save is written, and the file is written even
    /// when nothing changed or it could not be read. The settings take the change only when
    /// the write succeeds. Resolves with the write's outcome.
    pub fn save(
        &mut self,
        change: impl FnOnce(&mut Settings) + 'static,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<Result<(), String>> {
        let (done, result) = oneshot::channel();
        self.enqueue(Box::new(change), SaveKind::Explicit(done), cx);
        result
    }

    /// A folder picked in the native folder picker (`dialog:selectFolder`) goes to the front
    /// of `recentDirectories`, without waiting: see [`Self::update`], which never writes it
    /// over a `settings.json` that could not be read.
    pub fn add_recent_directory(&mut self, dir: &str, cx: &mut Context<Self>) {
        let dir = dir.to_owned();
        self.update(move |settings| settings.add_recent_directory(&dir), cx);
    }

    /// `ssh.saveConnection`: save a connection made in the picker's form, without waiting
    /// (see [`Self::update`]). It replaces, in place, the saved connection for the same
    /// `user@host:port`, new id and port forwards included, else it is added at the end.
    pub fn save_ssh_connection(&mut self, config: SshConnectionConfig, cx: &mut Context<Self>) {
        self.update(move |settings| settings.save_ssh_connection(config), cx);
    }

    /// `toggleSound`: flip the attention sound and save it, tracked in [`Self::sound_save`].
    /// Ignored while the last toggle is still saving.
    pub fn toggle_sound(&mut self, cx: &mut Context<Self>) {
        if self.sound == SoundSave::Saving {
            return;
        }
        self.sound = SoundSave::Saving;
        cx.notify();
        let saved = self.save(|s| s.notifications.sound_enabled = !s.notifications.sound_enabled, cx);
        self._sound_save = Some(cx.spawn(async move |this, cx| {
            let outcome = saved.await.unwrap_or_else(|_| Err("settings store closed".to_owned()));
            let shown = this.update(cx, |store, cx| {
                store.sound = match outcome {
                    Ok(()) => SoundSave::Idle,
                    Err(error) => SoundSave::Failed(error),
                };
                cx.notify();
            });
            if shown.is_err() {
                tracing::debug!("settings store closed before the sound preference was saved");
            }
        }));
    }

    fn enqueue(&mut self, change: Box<dyn FnOnce(&mut Settings)>, kind: SaveKind, cx: &mut Context<Self>) {
        let Some(dir) = self.user_data_dir.clone() else {
            // Nothing to write: in memory, a save always succeeds.
            let mut next = (*self.settings).clone();
            change(&mut next);
            self.apply(next, cx);
            kind.reply(Ok(()));
            return;
        };
        self.queue.push_back(QueuedSave { change, kind });
        if self.writer.is_some() {
            return;
        }
        self.writer = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some((next, kind))) = this.update(cx, |store, _| store.next_save()) else {
                    return;
                };
                let written = cx.background_executor().spawn({
                    let (dir, next) = (dir.clone(), next.clone());
                    async move { settings::save(&dir, &next) }
                });
                let result = written.await;
                let outcome = this
                    .update(cx, |store, cx| store.finish_save(&dir, next, result, cx))
                    .unwrap_or_else(|_| Err("settings store closed".to_owned()));
                kind.reply(outcome);
            }
        }));
    }

    /// The next save to write, with its change applied; `None` (and the writer done) when
    /// the queue is empty. A change made in passing is dropped (and logged) while
    /// `settings.json` could not be read.
    fn next_save(&mut self) -> Option<(Settings, SaveKind)> {
        loop {
            let Some(QueuedSave { change, kind }) = self.queue.pop_front() else {
                self.writer = None;
                return None;
            };
            match (&kind, self.on_disk) {
                (SaveKind::InPassing, OnDisk::Unreadable) => {
                    tracing::warn!(
                        error = ?self.error,
                        "settings.json could not be read; a change is not written over it"
                    );
                }
                (SaveKind::Explicit(_), OnDisk::Known | OnDisk::Unreadable)
                | (SaveKind::InPassing, OnDisk::Known) => {
                    let mut next = (*self.settings).clone();
                    change(&mut next);
                    return Some((next, kind));
                }
            }
        }
    }

    fn finish_save(
        &mut self,
        dir: &Path,
        next: Settings,
        result: Result<(), SettingsError>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match result {
            Ok(()) => {
                self.seen = modified(dir);
                self.error = None;
                self.on_disk = OnDisk::Known;
                self.apply(next, cx);
                Ok(())
            }
            Err(error) => {
                tracing::error!(%error, "failed to save settings");
                let message = error.to_string();
                self.error = Some(message.clone());
                cx.notify();
                Err(message)
            }
        }
    }

    fn apply(&mut self, next: Settings, cx: &mut Context<Self>) {
        if next != *self.settings {
            self.settings = Rc::new(next);
            cx.emit(SettingsEvent::Changed);
        }
        cx.notify();
    }
}
