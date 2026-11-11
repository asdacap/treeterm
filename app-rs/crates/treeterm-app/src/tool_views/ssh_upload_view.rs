//! The Upload tab and the session pane's SSH → Upload (`SshUploadPane.tsx`).

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FontWeight, Subscription, Window, div, px, rgb};
use treeterm_model::session_store::SessionStore;
use treeterm_model::tool_tabs::ssh_upload::{LinkStatus, SshUpload, UploadState, UploadTarget};

use crate::theme::colors;
use crate::ui::{DialogButton, FieldStyle, MONO, pane_dialog_button, field, normal_line};

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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, primary, border, field_bg) = (c.text_secondary, c.text_primary, c.border, c.bg_secondary);
        let model = self.model.read(cx);
        let (target, local, upload, can_upload) = (
            model.target(cx),
            model.local_path().map(|p| p.to_string_lossy().into_owned()),
            model.upload_state().clone(),
            model.can_upload(cx),
        );
        // `.ssh-pane-output`: padded 8px, monospace 12px, line-height 1.5.
        let root = v_flex()
            .id("ssh-upload")
            .size_full()
            .p(px(8.))
            .overflow_y_scroll()
            .font_family(MONO)
            .text_size(px(12.))
            .line_height(px(18.))
            .text_color(primary)
            .bg(c.bg_primary);
        let (name, status) = match target {
            UploadTarget::Local => {
                return root.child(div().text_color(secondary).child("File upload is only available for SSH sessions."));
            }
            UploadTarget::Gone => return root.child(div().text_color(secondary).child("The SSH connection is closed.")),
            UploadTarget::Remote { name, status, .. } => (name, status),
        };
        let uploading = upload == UploadState::Uploading;
        let banner = |text: String, color: gpui_kit::Hsla| h_flex().gap(px(6.)).text_color(color).child(text);
        let banner = match upload {
            UploadState::Idle => None,
            UploadState::Uploading => Some(
                h_flex()
                    .gap(px(6.))
                    .child(Spinner::new().with_size(px(14.)).color(primary))
                    .child("Uploading...")
                    .into_any_element(),
            ),
            UploadState::Done(path) => Some(banner(format!("Uploaded to {path}"), rgb(0x4caf50).into()).into_any_element()),
            UploadState::Failed(message) => {
                Some(banner(format!("Upload failed: {message}"), rgb(0xf44336).into()).into_any_element())
            }
        };
        let label = |text: &'static str| {
            div().text_size(px(11.)).line_height(px(16.5)).font_weight(FontWeight::SEMIBOLD).text_color(secondary).child(text)
        };
        // `.remote-dir-path-input`: 12px monospace on the secondary background.
        let style = FieldStyle {
            bg: field_bg,
            border,
            radius: 4.,
            pad: (4., 8.),
            size: 12.,
            mono: true,
            height: 24.,
        };
        let local_field = div()
            .flex_1()
            .min_w_0()
            .h(px(24.))
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(4.))
            .border_1()
            .border_color(border)
            .bg(field_bg)
            .line_height(normal_line(12.))
            .child(match local {
                Some(path) => div().truncate().child(path),
                None => div().text_color(rgb(0x757575)).child("No file selected"),
            });
        root.gap(px(12.))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("Upload to {name}")))
            .when_some(
                match status {
                    LinkStatus::Connected => None,
                    LinkStatus::Down(state) => Some(state),
                },
                |d, state| {
                    d.child(div().text_color(secondary).child(format!(
                        "Connection is {state} — uploads are disabled until connected."
                    )))
                },
            )
            .child(
                v_flex().gap(px(4.)).child(label("Local file")).child(
                    h_flex().gap(px(8.)).child(local_field).child(
                        pane_dialog_button("upload-choose", "Choose file...", DialogButton::Plain, uploading, cx)
                            .when(!uploading, |d| d.on_click(cx.listener(|view, _, _, cx| view.choose_file(cx)))),
                    ),
                ),
            )
            .child(
                v_flex().gap(px(4.)).child(label("Remote directory (absolute)")).child(
                    h_flex()
                        .gap(px(8.))
                        .child(div().flex_1().min_w_0().when(uploading, |d| d.opacity(0.5)).child(
                            field(&self.remote_dir, style, window, cx).disabled(uploading),
                        ))
                        .child(
                            pane_dialog_button("upload-browse", "Browse...", DialogButton::Plain, uploading, cx)
                                .when(!uploading, |d| d.on_click(cx.listener(|view, _, window, cx| view.browse(window, cx)))),
                        ),
                ),
            )
            .child(
                h_flex().gap(px(8.)).child(
                    pane_dialog_button("upload-start", "Upload", DialogButton::Primary, !can_upload, cx)
                        .when(can_upload, |d| d.on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.upload(cx))))),
                ),
            )
            .children(banner)
    }
}
