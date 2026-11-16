//! The macOS menu items AppKit implements and GPUI does not wrap (Electron gets them from
//! menu roles): the About panel, Bring All to Front, the Help menu's search field, and no
//! automatic "Enter Full Screen" item beside View > Toggle Full Screen.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAboutPanelOptionApplicationName, NSAboutPanelOptionApplicationVersion, NSAboutPanelOptionKey,
    NSApplication,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSUserDefaults};

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
