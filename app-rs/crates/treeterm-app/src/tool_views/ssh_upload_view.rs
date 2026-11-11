//! The Upload tab (`SshUploadPane.tsx`).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, Subscription, Window, div, rgb};
use treeterm_model::session_store::SessionStore;
use treeterm_model::tool_tabs::ssh_upload::{LinkStatus, SshUpload, UploadState, UploadTarget};

use crate::theme::colors;

pub struct SshUploadView {
    model: Entity<SshUpload>,
    /// Browses the host's directories.
    session: Entity<SessionStore>,
    remote_dir: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl SshUploadView {
    pub fn new(model: Entity<SshUpload>, session: Entity<SessionStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let start = model.read(cx).remote_dir(cx);
        let remote_dir = cx.new(|cx| InputState::new(window, cx).placeholder("/path/on/remote").default_value(start));
        let typed = cx.subscribe(&remote_dir, |view, input, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let text = input.read(cx).value().to_string();
                view.model.update(cx, |m, cx| m.set_remote_dir(&text, cx));
            }
        });
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self { model, session, remote_dir, _subscriptions: vec![typed, observe] }
    }

    /// "Choose file…": the native picker (local files).
    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Upload".into()),
        });
        cx.spawn(async move |view, cx| {
            let path = match picked.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => paths.remove(0),
                Ok(Ok(_)) => return, // cancelled
                Ok(Err(error)) => return tracing::error!(%error, "file picker failed"),
                Err(_canceled) => return,
            };
            match view.update(cx, |view, cx| view.model.update(cx, |m, cx| m.set_local_path(path, cx))) {
                Ok(()) | Err(_) => {} // Err: the tab closed
            }
        })
        .detach();
    }

    /// "Browse…": pick the directory on the host.
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (model, input) = (self.model.clone(), self.remote_dir.clone());
        let on_picked: crate::remote_dir_dialog::OnPicked = std::rc::Rc::new(move |path, window, cx| {
            input.update(cx, |input, cx| input.set_value(path.clone(), window, cx));
            model.update(cx, |m, cx| m.set_remote_dir(&path, cx));
        });
        crate::remote_dir_dialog::open(self.session.clone(), on_picked, window, cx);
    }
}

impl Render for SshUploadView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, error) = (c.text_secondary, c.error);
        let model = self.model.read(cx);
        let (target, local, upload, can_upload) = (
            model.target(cx),
            model.local_path().map(|p| p.to_string_lossy().into_owned()),
            model.upload_state().clone(),
            model.can_upload(cx),
        );
        let root = v_flex().id("ssh-upload").size_full().p_4().gap_3().overflow_y_scroll();
        let (name, status) = match target {
            UploadTarget::Local => {
                return root.child(div().text_color(secondary).child("File upload is only available for SSH sessions."));
            }
            UploadTarget::Gone => return root.child(div().text_color(secondary).child("The SSH connection is closed.")),
            UploadTarget::Remote { name, status, .. } => (name, status),
        };
        let uploading = upload == UploadState::Uploading;
        let banner = match upload {
            UploadState::Idle => None,
            UploadState::Uploading => {
                Some(h_flex().gap_2().text_sm().child(Spinner::new()).child("Uploading...").into_any_element())
            }
            UploadState::Done(path) => {
                Some(div().text_sm().text_color(rgb(0x4caf50)).child(format!("Uploaded to {path}")).into_any_element())
            }
            UploadState::Failed(message) => {
                Some(div().text_sm().text_color(error).child(format!("Upload failed: {message}")).into_any_element())
            }
        };
        let label = |text: &'static str| div().text_xs().text_color(secondary).child(text);
        root.child(div().text_base().child(format!("Upload to {name}")))
            .when_some(
                match status {
                    LinkStatus::Connected => None,
                    LinkStatus::Down(state) => Some(state),
                },
                |d, state| {
                    d.child(div().text_sm().text_color(secondary).child(format!(
                        "Connection is {state} — uploads are disabled until connected."
                    )))
                },
            )
            .child(
                v_flex().gap_1().child(label("Local file")).child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_sm()
                                .when(local.is_none(), |d| d.text_color(secondary))
                                .child(local.unwrap_or_else(|| "No file selected".to_owned())),
                        )
                        .child(
                            Button::new("upload-choose")
                                .small()
                                .outline()
                                .disabled(uploading)
                                .label("Choose file...")
                                .on_click(cx.listener(|view, _, _, cx| view.choose_file(cx))),
                        ),
                ),
            )
            .child(
                v_flex().gap_1().child(label("Remote directory (absolute)")).child(
                    h_flex()
                        .gap_2()
                        .child(div().flex_1().child(Input::new(&self.remote_dir).small().disabled(uploading)))
                        .child(
                            Button::new("upload-browse")
                                .small()
                                .outline()
                                .disabled(uploading)
                                .label("Browse...")
                                .on_click(cx.listener(|view, _, window, cx| view.browse(window, cx))),
                        ),
                ),
            )
            .child(
                h_flex().child(
                    Button::new("upload-start")
                        .small()
                        .primary()
                        .disabled(!can_upload)
                        .label("Upload")
                        .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.upload(cx)))),
                ),
            )
            .children(banner)
    }
}
