//! The native open panel behind Electron's `dialog:selectFolder` and `dialog:selectFile`:
//! `dialog.showOpenDialog(focusedWindow, { properties: ['openDirectory' | 'openFile'] })`.

use std::path::PathBuf;

use gpui_kit::{App, PathPromptOptions, Task, Window};

/// What the panel chooses: Electron's `openDirectory` or `openFile` property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choose {
    Directory,
    File,
}

/// Show the panel for `window`, with the platform's default "Open" button (Electron sets no
/// `buttonLabel`). On macOS it is a sheet on the window, as Electron attaches it to the
/// focused window; elsewhere it is GPUI's picker. Resolves to the chosen path; to None when
/// the panel is cancelled, or fails (logged).
#[cfg_attr(
    not(target_os = "macos"),
    expect(
        unused_variables,
        reason = "only macOS attaches the panel to the window"
    )
)]
pub fn choose_path(choose: Choose, window: &Window, cx: &App) -> Task<Option<PathBuf>> {
    #[cfg(target_os = "macos")]
    match crate::macos::ns_window(window) {
        Ok(parent) => {
            return cx.foreground_executor().spawn(async move {
                match crate::macos::open_panel_sheet(parent, choose).await {
                    Ok(path) => path,
                    Err(error) => {
                        tracing::error!(%error, ?choose, "open panel failed");
                        None
                    }
                }
            });
        }
        Err(error) => tracing::error!(%error, "the open panel is not a sheet on this window"),
    }
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: choose == Choose::File,
        directories: choose == Choose::Directory,
        multiple: false,
        prompt: None,
    });
    cx.foreground_executor().spawn(async move {
        match picked.await {
            Ok(Ok(paths)) => paths.and_then(|paths| paths.into_iter().next()),
            Ok(Err(error)) => {
                tracing::error!(%error, ?choose, "open panel failed");
                None
            }
            Err(_canceled) => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui_kit::TestAppContext;

    use super::{Choose, choose_path};

    /// Electron's `properties` and no `buttonLabel`: one file or one folder, with the
    /// platform's default "Open" button. A test window has no AppKit window to attach a sheet
    /// to, so the panel is GPUI's, which the test platform answers.
    #[gpui_kit::test]
    async fn the_panel_chooses_one_path_with_the_default_button(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let cases = [
            (Choose::File, true, false, "/home/me/notes.txt"),
            (Choose::Directory, false, true, "/home/me/src"),
        ];
        for (choose, files, directories, chosen) in cases {
            let picked = cx.update(|window, cx| choose_path(choose, window, cx));
            assert!(cx.did_prompt_for_paths(), "{choose:?}");
            cx.simulate_path_prompt_response(|options| {
                assert_eq!(options.files, files, "{choose:?}");
                assert_eq!(options.directories, directories, "{choose:?}");
                assert!(!options.multiple, "{choose:?}");
                assert_eq!(options.prompt, None, "{choose:?}");
                Some(vec![PathBuf::from(chosen)])
            });
            assert_eq!(picked.await, Some(PathBuf::from(chosen)), "{choose:?}");
        }
    }

    #[gpui_kit::test]
    async fn a_cancelled_panel_chooses_nothing(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let picked = cx.update(|window, cx| choose_path(Choose::File, window, cx));
        cx.simulate_path_prompt_response(|_| None);
        assert_eq!(picked.await, None);
    }
}
