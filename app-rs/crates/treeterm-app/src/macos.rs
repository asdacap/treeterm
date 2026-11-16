//! The macOS menu items AppKit implements and GPUI does not wrap (Electron gets them from
//! menu roles): the About panel, Bring All to Front, the Help menu's search field, and no
//! automatic "Enter Full Screen" item beside View > Toggle Full Screen. Also the open panel
//! as a sheet on its window, which GPUI's free-standing `prompt_for_paths` is not.

use std::cell::Cell;
use std::path::PathBuf;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_app_kit::{
    NSAboutPanelOptionApplicationName, NSAboutPanelOptionApplicationVersion, NSAboutPanelOptionKey,
    NSApplication, NSModalResponse, NSModalResponseAbort, NSModalResponseOK, NSOpenPanel, NSView,
    NSWindow,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSUserDefaults};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::open_panel::Choose;

/// The shared application. GPUI calls the app back on the main thread, as AppKit requires.
fn app() -> Retained<NSApplication> {
    let main_thread = MainThreadMarker::new().expect("AppKit is called from the main thread");
    NSApplication::sharedApplication(main_thread)
}

/// TreeTerm > About TreeTerm: the standard About panel (Electron's `about` role), with the
/// name and version also when the binary runs outside a bundle.
pub fn about_panel() {
    let name = NSString::from_str("TreeTerm");
    let version = NSString::from_str(env!("CARGO_PKG_VERSION"));
    // SAFETY: AppKit's constant strings, linked with AppKit.
    let keys: [&NSAboutPanelOptionKey; 2] = unsafe {
        [
            NSAboutPanelOptionApplicationName,
            NSAboutPanelOptionApplicationVersion,
        ]
    };
    let values: [&AnyObject; 2] = [&name, &version];
    let options = NSDictionary::from_slices(&keys, &values);
    // SAFETY: both options take a string.
    unsafe { app().orderFrontStandardAboutPanelWithOptions(&options) };
}

/// Window > Bring All to Front (Electron's `front` role).
pub fn bring_all_to_front() {
    app().arrangeInFront(None);
}

/// AppKit adds its own "Enter Full Screen" item to the menus unless told not to; Electron
/// turns it off too, since View > Toggle Full Screen does the same. Call before the
/// application finishes launching. Registered as a default, so nothing is written to the
/// user's preferences.
pub fn disable_auto_full_screen_item() {
    let key = NSString::from_str("NSFullScreenMenuItemEverywhere");
    let off = NSNumber::new_bool(false);
    let values: [&AnyObject; 1] = [&off];
    let defaults = NSDictionary::from_slices(&[&*key], &values);
    // SAFETY: the value is a boolean NSNumber, as AppKit reads this default.
    unsafe { NSUserDefaults::standardUserDefaults().registerDefaults(&defaults) };
}

/// Make the menu named "Help" the application's Help menu (Electron's `help` role), so
/// macOS adds its search field to it. Call after `set_menus`.
pub fn register_help_menu() {
    let app = app();
    let help = app
        .mainMenu()
        .and_then(|menu| menu.itemWithTitle(&NSString::from_str("Help")))
        .and_then(|item| item.submenu());
    match help {
        Some(menu) => app.setHelpMenu(Some(&menu)),
        None => tracing::error!("the menu bar has no Help menu"),
    }
}

/// The AppKit window behind a GPUI window. A test window has none.
pub fn ns_window(window: &gpui_kit::Window) -> Result<Retained<NSWindow>, String> {
    let handle = HasWindowHandle::window_handle(window).map_err(|error| error.to_string())?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err(format!("not an AppKit window: {:?}", handle.as_raw()));
    };
    // SAFETY: GPUI's macOS handle points at the window's content NSView, which lives as long
    // as the window, borrowed here.
    let view = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    view.window()
        .ok_or_else(|| "the window's view is in no NSWindow".to_owned())
}

/// Electron's `dialog.showOpenDialog(focusedWindow, { properties: [choose] })`: an
/// NSOpenPanel attached to `parent` as a sheet. Electron sets only what it chooses, so the
/// rest keeps AppKit's defaults: the "Open" button, no New Folder, aliases resolved. Resolves
/// to the first chosen path; to None when cancelled.
///
/// Await it in a spawned task, not inside a GPUI update: the sheet takes the key window from
/// `parent`, which GPUI handles as a window event.
pub async fn open_panel_sheet(
    parent: Retained<NSWindow>,
    choose: Choose,
) -> Result<Option<PathBuf>, String> {
    // AppKit can return nil despite the nonnull annotation of objc2's binding (as GPUI's own
    // panel notes).
    // SAFETY: `+[NSOpenPanel openPanel]` takes no arguments and returns an NSOpenPanel.
    let panel: Option<Retained<NSOpenPanel>> = unsafe { msg_send![class!(NSOpenPanel), openPanel] };
    let panel = panel.ok_or("macOS could not create the open panel")?;
    panel.setCanChooseFiles(choose == Choose::File);
    panel.setCanChooseDirectories(choose == Choose::Directory);

    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let done_tx = Cell::new(Some(done_tx));
    let handler = RcBlock::new({
        let panel = panel.clone();
        move |response: NSModalResponse| {
            let Some(done_tx) = done_tx.take() else {
                return;
            };
            let outcome = if response == NSModalResponseOK {
                let urls = panel.URLs();
                Ok(urls
                    .iter()
                    .filter(|url| url.isFileURL())
                    .find_map(|url| url.to_file_path()))
            } else if response == NSModalResponseAbort {
                Err("macOS could not show the open panel".to_owned())
            } else {
                Ok(None) // cancelled
            };
            match done_tx.send(outcome) {
                Ok(()) | Err(_) => {} // Err: the task awaiting the panel was dropped
            }
        }
    });
    panel.beginSheetModalForWindow_completionHandler(&parent, &handler);
    match done_rx.await {
        Ok(outcome) => outcome,
        Err(_closed) => Err("the open panel closed without an answer".to_owned()),
    }
}
