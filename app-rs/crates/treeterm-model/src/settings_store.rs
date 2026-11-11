//! `settings.json` as an entity: the current settings, saved on change, and reloaded when
//! another writer (the Electron app, or a hand edit) changes the file.
//!
//! Port of the renderer's settings store plus main's load/save. The file is shared with the
//! Electron app, so it is polled for changes (by modification time) rather than owned.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use gpui_kit::{Context, EventEmitter, Task};
use treeterm_core::settings::{self, Settings, SettingsError};

/// How often the file is checked for edits made elsewhere.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsEvent {
    Changed,
}

impl EventEmitter<SettingsEvent> for SettingsStore {}

pub struct SettingsStore {
    /// `None` when there is no user data dir (settings stay in memory).
    user_data_dir: Option<PathBuf>,
    settings: Rc<Settings>,
    /// The file's modification time when last read or written by us.
    seen: Option<SystemTime>,
    /// The last load or save failure, shown to the user.
    error: Option<String>,
    _poll: Option<Task<()>>,
}

fn modified(dir: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(settings::settings_path(dir)).and_then(|m| m.modified()).ok()
}

impl SettingsStore {
    /// Load from `user_data_dir` and start watching. A missing file is the defaults; an
    /// unreadable or corrupt one keeps the defaults and records the error.
    pub fn new(user_data_dir: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let mut store = Self {
            user_data_dir,
            settings: Rc::new(Settings::default()),
            seen: None,
            error: None,
            _poll: None,
        };
        store.reload();
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
            _poll: None,
        }
    }

    pub fn settings(&self) -> &Rc<Settings> {
        &self.settings
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
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

    fn check_for_edits(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = &self.user_data_dir else {
            return;
        };
        if modified(dir) == self.seen {
            return;
        }
        if self.reload() {
            cx.emit(SettingsEvent::Changed);
        }
        cx.notify();
    }

    /// Change settings and persist them. A failed save keeps the change in memory and
    /// records the error.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let mut next = (*self.settings).clone();
        change(&mut next);
        if next == *self.settings {
            return;
        }
        if let Some(dir) = &self.user_data_dir {
            match settings::save(dir, &next) {
                Ok(()) => {
                    self.error = None;
                    self.seen = modified(dir);
                }
                Err(error) => {
                    let error: SettingsError = error;
                    tracing::error!(%error, "failed to save settings");
                    self.error = Some(error.to_string());
                }
            }
        }
        self.settings = Rc::new(next);
        cx.emit(SettingsEvent::Changed);
        cx.notify();
    }
}
