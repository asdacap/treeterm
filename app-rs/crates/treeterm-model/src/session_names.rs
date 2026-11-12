//! Per-session sidebar state (`sessionNames.ts`): custom names (renaming a session in the
//! sidebar), the order sessions were dragged into, and the directories recently opened as
//! workspaces (`recentDirectories.ts`), kept in `ui-state.json` by connection target so they
//! survive reconnects and restarts.

use std::path::PathBuf;

use gpui_kit::Context;
use treeterm_core::connection::ConnectionTarget;
use treeterm_core::settings::MAX_RECENT_DIRECTORIES;
use treeterm_core::ui_state::{self, UiState};

pub struct SessionNames {
    /// `None`: nowhere to persist (no config dir); renames last for this run.
    path: Option<PathBuf>,
    state: UiState,
}

/// Where a dragged session lands relative to the one it is dropped on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
}

/// "Local", or the ssh label / `user@host` (`deriveDefaultSessionName`).
pub fn default_name(target: &ConnectionTarget) -> String {
    match target {
        ConnectionTarget::Local => "Local".to_owned(),
        ConnectionTarget::Remote(config) => config.display_name(),
    }
}

impl SessionNames {
    pub fn new(path: Option<PathBuf>) -> Self {
        let state = match path.as_deref().map(ui_state::load) {
            Some(Ok(state)) => state,
            // Logged, not overwritten: saving would drop whatever the file holds.
            Some(Err(error)) => {
                tracing::error!(%error, "UI state not loaded; session names start empty");
                return Self { path: None, state: UiState::default() };
            }
            None => UiState::default(),
        };
        Self { path, state }
    }

    /// The custom name, else the default.
    pub fn name(&self, target: &ConnectionTarget) -> String {
        self.state
            .session_names
            .get(&ui_state::session_key(target))
            .cloned()
            .unwrap_or_else(|| default_name(target))
    }

    /// A blank name, or the default one, clears the custom name.
    pub fn rename(&mut self, target: &ConnectionTarget, name: &str, cx: &mut Context<Self>) {
        let key = ui_state::session_key(target);
        let name = name.trim();
        if name.is_empty() || name == default_name(target) {
            self.state.session_names.remove(&key);
        } else {
            self.state.session_names.insert(key, name.to_owned());
        }
        self.save();
        cx.notify();
    }

    /// Put `items` in sidebar order (`getSortedIds`): sessions placed by a drag first, as
    /// placed; the others after them, keeping their order.
    pub fn sort<T>(&self, items: &mut [T], target: impl Fn(&T) -> &ConnectionTarget) {
        let order = &self.state.session_order;
        items.sort_by_cached_key(|item| {
            let key = ui_state::session_key(target(item));
            order.iter().position(|placed| *placed == key).unwrap_or(usize::MAX)
        });
    }

    /// Drag and drop in the sidebar (`reorderSession`). `shown` is every session on screen:
    /// each gets a place, so one never dragged before can be dragged or dropped on too.
    pub fn reorder(
        &mut self,
        shown: &[ConnectionTarget],
        dragged: &ConnectionTarget,
        onto: &ConnectionTarget,
        side: Side,
        cx: &mut Context<Self>,
    ) {
        let (dragged, onto) = (ui_state::session_key(dragged), ui_state::session_key(onto));
        if dragged == onto {
            return;
        }
        let order = &mut self.state.session_order;
        // Appended in the order given: unplaced sessions already show in that order, after
        // the placed ones, so the list stays what the sidebar shows.
        for key in shown.iter().map(ui_state::session_key) {
            if !order.contains(&key) {
                order.push(key);
            }
        }
        order.retain(|key| *key != dragged);
        let at = order.iter().position(|key| *key == onto).map_or(order.len(), |at| match side {
            Side::Before => at,
            Side::After => at + 1,
        });
        order.insert(at, dragged);
        self.save();
        cx.notify();
    }

    /// The directories opened as workspaces on `target`, most recent first.
    pub fn recent_directories(&self, target: &ConnectionTarget) -> &[String] {
        self.state.recent_directories.get(&ui_state::session_key(target)).map_or(&[], Vec::as_slice)
    }

    /// `addRecent`: `path` moves to the front; the oldest beyond the limit drop off.
    pub fn add_recent_directory(&mut self, target: &ConnectionTarget, path: &str, cx: &mut Context<Self>) {
        let recent = self.state.recent_directories.entry(ui_state::session_key(target)).or_default();
        recent.retain(|existing| existing != path);
        recent.insert(0, path.to_owned());
        recent.truncate(MAX_RECENT_DIRECTORIES);
        self.save();
        cx.notify();
    }

    fn save(&self) {
        if let Some(path) = &self.path
            && let Err(error) = ui_state::save(path, &self.state)
        {
            tracing::error!(%error, "session names not saved");
        }
    }
}
