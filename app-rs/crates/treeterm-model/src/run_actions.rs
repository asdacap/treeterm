//! The workspace header's "Run" menu (`RunActionDropdown.tsx` + `createRunActionsStore`):
//! npm scripts, make targets, just recipes, Taskfile tasks and VS Code tasks found in the
//! workspace, each started in a new terminal tab.

use std::sync::Arc;

use gpui_kit::{Context, Entity, Task, WeakEntity};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::run_actions::{RunAction, RunActionsClient};

use crate::workspace_store::WorkspaceStore;

#[derive(Debug, Clone)]
pub enum Detection {
    Detecting,
    Done(Vec<RunAction>),
}

pub struct RunActions {
    client: RunActionsClient,
    path: String,
    workspace: WeakEntity<WorkspaceStore>,
    detection: Detection,
    /// Why the last run failed to start.
    error: Option<String>,
    _detect: Option<Task<()>>,
    _run: Option<Task<()>>,
}

impl RunActions {
    pub fn new(conn: &DaemonConnection, workspace: &Entity<WorkspaceStore>, cx: &mut Context<Self>) -> Self {
        let client = RunActionsClient::new(Arc::new(conn.fs_client()), Arc::new(conn.clone()));
        let path = workspace.read(cx).workspace().path.clone();
        let mut model = Self {
            client,
            path,
            workspace: workspace.downgrade(),
            detection: Detection::Detecting,
            error: None,
            _detect: None,
            _run: None,
        };
        model.detect(cx);
        model
    }

    pub fn detection(&self) -> &Detection {
        &self.detection
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Look for actions again (files may have changed).
    pub fn detect(&mut self, cx: &mut Context<Self>) {
        self.detection = Detection::Detecting;
        let (client, path) = (self.client.clone(), self.path.clone());
        self._detect = Some(cx.spawn(async move |this, cx| {
            let detected = client.detect(&path).await;
            for failure in &detected.failures {
                tracing::warn!(source = failure.source.as_str(), error = %failure.error, "run actions");
            }
            match this.update(cx, |model, cx| {
                model.detection = Detection::Done(detected.actions);
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: workspace gone
            }
        }));
        cx.notify();
    }

    /// Start `action` in a new PTY and show it as a terminal tab titled with the action id.
    pub fn run(&mut self, action: &RunAction, cx: &mut Context<Self>) {
        let (client, path, id) = (self.client.clone(), self.path.clone(), action.id());
        let workspace = self.workspace.clone();
        self._run = Some(cx.spawn(async move |this, cx| {
            let result = client.run(&path, &id).await.map_err(|e| e.to_string());
            let opened = match result {
                Ok(pty_id) => workspace
                    .update(cx, |w, cx| {
                        let state = serde_json::json!({ "ptyId": pty_id, "ptyHandle": null, "keepOnExit": true });
                        match w.add_tab_with("terminal", state, cx) {
                            Some(tab) => {
                                w.update_tab_title(&tab, &id, cx);
                                Ok(())
                            }
                            None => Err("No terminal application".to_owned()),
                        }
                    })
                    .unwrap_or_else(|_| Err("The workspace went away".to_owned())),
                Err(error) => Err(format!("Failed to run {id}: {error}")),
            };
            match this.update(cx, |model, cx| {
                model.error = opened.err();
                cx.notify();
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
    }
}
