//! Custom session names persist in `ui-state.json` and fall back to the default name.

use gpui_kit::{AppContext as _, TestAppContext};
use treeterm_core::connection::ConnectionTarget;
use treeterm_model::session_names::SessionNames;

#[gpui_kit::test]
fn a_rename_persists_and_a_blank_name_restores_the_default(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ui-state.json");
    let names = cx.new(|_| SessionNames::new(Some(path.clone())));
    let local = ConnectionTarget::Local;
    assert_eq!(names.read_with(cx, |n, _| n.name(&local)), "Local");

    names.update(cx, |n, cx| n.rename(&local, "  Laptop ", cx));
    assert_eq!(names.read_with(cx, |n, _| n.name(&local)), "Laptop");
    let reloaded = SessionNames::new(Some(path.clone()));
    assert_eq!(reloaded.name(&local), "Laptop", "saved to disk");

    names.update(cx, |n, cx| n.rename(&local, "   ", cx));
    assert_eq!(SessionNames::new(Some(path.clone())).name(&local), "Local");

    names.update(cx, |n, cx| n.rename(&local, "Local", cx));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("sessionNames"), "the default name is not stored: {text}");
}

#[gpui_kit::test]
fn a_corrupt_file_is_left_alone(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ui-state.json");
    std::fs::write(&path, "{broken").unwrap();
    let names = cx.new(|_| SessionNames::new(Some(path.clone())));
    names.update(cx, |n, cx| n.rename(&ConnectionTarget::Local, "Laptop", cx));
    assert_eq!(names.read_with(cx, |n, _| n.name(&ConnectionTarget::Local)), "Laptop");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
}
