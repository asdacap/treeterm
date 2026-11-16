//! `settings.json` as an entity: the current settings, saved on change, and reloaded when
//! another writer (the Electron app, or a hand edit) changes the file.
//!
//! Port of the renderer's settings store plus main's load/save. The file is shared with the
//! Electron app, so it is polled for changes (by modification time) rather than owned.
//!
//! Saves are written in the background, one at a time in the order they were asked for, and
//! reach [`SettingsStore::settings`] only once written: a failed save changes nothing but the
//! error (Electron's `saveSettings` and `toggleSound` set the settings only on success).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use futures::channel::oneshot;
use gpui_kit::{Context, EventEmitter, Task};
use treeterm_core::settings::{self, Settings, SettingsError};

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

/// A change waiting to be written, applied to the settings as they are when its turn comes.
struct QueuedSave {
    change: Box<dyn FnOnce(&mut Settings)>,
    /// Told the write's outcome; `None` when nobody waits for it.
    done: Option<SaveReply>,
}

pub struct SettingsStore {
    /// `None` when there is no user data dir (settings stay in memory).
    user_data_dir: Option<PathBuf>,
    settings: Rc<Settings>,
    /// The file's modification time when last read or written by us.
    seen: Option<SystemTime>,
    /// The last load or save failure.
    error: Option<String>,
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
    /// defaults (and the file) and records the error.
    pub fn new(user_data_dir: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let mut store = Self {
            user_data_dir,
            settings: Rc::new(Settings::default()),
            seen: None,
            error: None,
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
                if *self.settings == loaded {
                    return false;
                }
                self.settings = Rc::new(loaded);
                true
            }
            Err(error) => {
                tracing::error!(%error, "settings unreadable; keeping the current settings");
                self.error = Some(error.to_string());
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
    /// is logged and recorded in [`Self::error`].
    pub fn update(&mut self, change: impl FnOnce(&mut Settings) + 'static, cx: &mut Context<Self>) {
        self.enqueue(Box::new(change), None, cx);
    }

    /// Change settings and persist them (`saveSettings`). The change is applied to the
    /// settings as they are once every earlier save is written, and the file is written even
    /// when nothing changed. The settings take the change only when the write succeeds.
    /// Resolves with the write's outcome.
    pub fn save(
        &mut self,
        change: impl FnOnce(&mut Settings) + 'static,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<Result<(), String>> {
        let (done, result) = oneshot::channel();
        self.enqueue(Box::new(change), Some(done), cx);
        result
    }

    /// A folder picked in the native folder picker (`dialog:selectFolder`) goes to the front
    /// of `recentDirectories`, without waiting: see [`Self::update`]. Skipped (and logged)
    /// while [`Self::error`] says `settings.json` failed to load or save, so a pick never
    /// writes the settings held in its place over a file that could not be read.
    pub fn add_recent_directory(&mut self, dir: &str, cx: &mut Context<Self>) {
        if let Some(error) = &self.error {
            tracing::warn!(%error, dir, "settings.json is in error; the picked folder is not recorded");
            return;
        }
        let dir = dir.to_owned();
        self.update(move |settings| settings.add_recent_directory(&dir), cx);
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

    fn enqueue(
        &mut self,
        change: Box<dyn FnOnce(&mut Settings)>,
        done: Option<SaveReply>,
        cx: &mut Context<Self>,
    ) {
        let Some(dir) = self.user_data_dir.clone() else {
            // Nothing to write: in memory, a save always succeeds.
            let mut next = (*self.settings).clone();
            change(&mut next);
            self.apply(next, cx);
            if let Some(done) = done {
                done.send(Ok(())).ok(); // the caller may not wait
            }
            return;
        };
        self.queue.push_back(QueuedSave { change, done });
        if self.writer.is_some() {
            return;
        }
        self.writer = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some((next, done))) = this.update(cx, |store, _| store.next_save()) else {
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
                if let Some(done) = done {
                    done.send(outcome).ok(); // the caller may not wait
                }
            }
        }));
    }

    /// The front of the queue with its change applied; `None` (and the writer done) when the
    /// queue is empty.
    fn next_save(&mut self) -> Option<(Settings, Option<SaveReply>)> {
        let Some(QueuedSave { change, done }) = self.queue.pop_front() else {
            self.writer = None;
            return None;
        };
        let mut next = (*self.settings).clone();
        change(&mut next);
        Some((next, done))
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
