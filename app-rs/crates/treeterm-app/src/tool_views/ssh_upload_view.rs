//! The Upload tab and the session pane's SSH → Upload (`SshUploadPane.tsx`).

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, FontWeight, Subscription, Window, div, px, rgb};
use treeterm_model::tool_tabs::ssh_upload::{LinkStatus, SshUpload, SshUploadEvent, UploadState, UploadTarget};

use crate::remote_dir_browser::{Inline, RemoteDirBrowserEvent};
use crate::theme::colors;
use crate::ui::{DialogButton, FieldStyle, MONO, field, pane_dialog_button};

pub struct SshUploadView {
    model: Entity<SshUpload>,
    /// The chosen file, read-only: it can be selected and scrolled, not typed into.
    local_path: Entity<InputState>,
    remote_dir: Entity<InputState>,
    /// The directory browser under the remote directory, shown by "Browse...".
    browser: Inline,
    _subscriptions: Vec<Subscription>,
}

/// The local field's text (`localPath ?? ''`).
fn chosen_file(model: &Entity<SshUpload>, cx: &App) -> String {
    model.read(cx).local_path().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default()
}

impl SshUploadView {
    pub fn new(model: Entity<SshUpload>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let start = model.read(cx).remote_dir(cx);
        let chosen = chosen_file(&model, cx);
        let local_path = cx.new(|cx| InputState::new(window, cx).placeholder("No file selected").default_value(chosen));
        let remote_dir = cx.new(|cx| InputState::new(window, cx).placeholder("/path/on/remote").default_value(start));
        let typed = cx.subscribe(&remote_dir, |view, input, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let text = input.read(cx).value().to_string();
                view.model.update(cx, |m, cx| m.set_remote_dir(&text, cx));
            }
        });
        let observe = cx.observe_in(&model, window, |view, model, window, cx| {
            let chosen = chosen_file(&model, cx);
            if view.local_path.read(cx).value() != chosen {
                view.local_path.update(cx, |input, cx| input.set_value(chosen, window, cx));
            }
            cx.notify();
        });
        // An emptied directory refilled at a connection status change.
        let refilled = cx.subscribe_in(&model, window, |view, _, event: &SshUploadEvent, window, cx| match event {
            SshUploadEvent::RemoteDirRefilled(dir) => {
                view.remote_dir.update(cx, |input, cx| input.set_value(dir.clone(), window, cx));
            }
        });
        Self { model, local_path, remote_dir, browser: Inline::Hidden, _subscriptions: vec![typed, observe, refilled] }
    }

    /// "Choose file…": the native picker (local files), with its default "Open" button.
    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
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

    /// "Browse…": the host's directories under the remote directory, from the typed one (else
    /// the home directory). Select sets the remote directory and hides the browser; while it
    /// is shown, "Browse…" does nothing. It is disabled while the connection is down.
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.browser {
            Inline::Shown { .. } => {}
            Inline::Hidden => {
                let Some(fs) = self.model.read(cx).fs(cx) else {
                    return;
                };
                let start = self.model.read(cx).browse_start(cx);
                self.browser = Inline::show(fs, start, window, cx, |view: &mut Self, event, window, cx| {
                    if let RemoteDirBrowserEvent::Selected(path) = event {
                        view.remote_dir.update(cx, |input, cx| input.set_value(path.clone(), window, cx));
                        view.model.update(cx, |m, cx| m.set_remote_dir(path, cx));
                    }
                    view.browser = Inline::Hidden;
                    cx.notify();
                });
                cx.notify();
            }
        }
    }
}

impl Render for SshUploadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, primary, border, field_bg) = (c.text_secondary, c.text_primary, c.border, c.bg_secondary);
        let model = self.model.read(cx);
        let (target, upload, can_upload) = (model.target(cx), model.upload_state().clone(), model.can_upload(cx));
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
        // There is no host to browse while the connection is down.
        let browsable = !uploading && status == LinkStatus::Connected;
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
        // `<input readOnly>`: the whole path can be selected, copied and scrolled to its end.
        let local_field = div().flex_1().min_w_0().child(field(&self.local_path, style, window, cx).readonly(true));
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
                            pane_dialog_button("upload-browse", "Browse...", DialogButton::Plain, !browsable, cx)
                                .when(browsable, |d| d.on_click(cx.listener(|view, _, window, cx| view.browse(window, cx)))),
                        ),
                ),
            )
            .children(self.browser.view())
            .child(
                h_flex().gap(px(8.)).child(
                    pane_dialog_button("upload-start", "Upload", DialogButton::Primary, !can_upload, cx)
                        .when(can_upload, |d| d.on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.upload(cx))))),
                ),
            )
            .children(banner)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, VisualTestContext};

    use super::*;

    #[gpui_kit::test]
    fn a_refilled_directory_shows_in_its_field(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(crate::theme::install);
        let model = cx.new(|cx| SshUpload::new(None, cx));
        let (view, cx) = cx.add_window_view({
            let model = model.clone();
            move |window, cx| SshUploadView::new(model, window, cx)
        });
        let field = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| view.read(cx).remote_dir.read(cx).value().to_string())
        };
        assert_eq!(field(cx), "");

        cx.update(|_, cx| {
            model.update(cx, |_, cx| cx.emit(SshUploadEvent::RemoteDirRefilled("/home/me".to_owned())));
        });
        cx.run_until_parked();
        assert_eq!(field(cx), "/home/me");
    }
}
