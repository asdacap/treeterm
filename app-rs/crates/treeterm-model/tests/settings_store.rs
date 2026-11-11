//! SettingsStore: save on change, reload on external edits, propagate to live terminals.

use gpui_kit::{AppContext as _, TestAppContext};
use treeterm_model::settings_store::SettingsStore;

#[gpui_kit::test]
fn changes_are_saved_and_external_edits_reload(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_owned();
    let store = cx.new(|cx| SettingsStore::new(Some(path.clone()), cx));
    store.read_with(cx, |s, _| assert_eq!(s.settings().terminal.max_cols, 160));

    store.update(cx, |s, cx| s.update(|settings| settings.terminal.max_cols = 120, cx));
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved["terminal"]["maxCols"], 120);

    // Another app edits the file.
    let mut edited = saved.clone();
    edited["terminal"]["maxCols"] = 90.into();
    // Make sure the modification time moves even on coarse-grained filesystems.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(path.join("settings.json"), serde_json::to_vec_pretty(&edited).unwrap()).unwrap();
    cx.executor().advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    store.read_with(cx, |s, _| assert_eq!(s.settings().terminal.max_cols, 90));
}

#[gpui_kit::test]
fn a_corrupt_file_keeps_the_settings_and_reports(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("settings.json"), b"{nope").unwrap();
    let path = dir.path().to_owned();
    let store = cx.new(|cx| SettingsStore::new(Some(path), cx));
    store.read_with(cx, |s, _| {
        assert!(s.error().is_some());
        assert_eq!(s.settings().terminal.max_cols, 160);
    });
}
