//! Custom session names persist in `ui-state.json` and fall back to the default name.

use gpui_kit::{AppContext as _, TestAppContext};
use treeterm_core::connection::ConnectionTarget;
use treeterm_model::session_names::{SessionNames, Side};

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

fn remote(host: &str) -> ConnectionTarget {
    use treeterm_core::settings::{ConnectionLabel, Extra, IdentityFile, SshConnectionConfig};
    ConnectionTarget::Remote(std::sync::Arc::new(SshConnectionConfig {
        id: host.to_owned(),
        host: host.to_owned(),
        user: "me".to_owned(),
        port: 22,
        identity_file: IdentityFile::SshDefault,
        label: ConnectionLabel::UserAtHost,
        port_forwards: Vec::new(),
        extra: Extra::new(),
    }))
}

fn names_of(names: &SessionNames, mut targets: Vec<ConnectionTarget>) -> Vec<String> {
    names.sort(&mut targets, |target| target);
    targets.iter().map(|target| names.name(target)).collect()
}

#[gpui_kit::test]
fn dragged_sessions_keep_their_place_and_the_rest_follow(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ui-state.json");
    let names = cx.new(|_| SessionNames::new(Some(path.clone())));
    let (local, a, b, c) = (ConnectionTarget::Local, remote("a"), remote("b"), remote("c"));
    let shown = vec![local.clone(), a.clone(), b.clone()];
    // Never dragged: the order given.
    assert_eq!(names.read_with(cx, |n, _| names_of(n, shown.clone())), ["Local", "me@a", "me@b"]);

    // An unnamed session never dragged before can be dragged, after or before another.
    names.update(cx, |n, cx| n.reorder(&shown, &local, &b, Side::After, cx));
    assert_eq!(names.read_with(cx, |n, _| names_of(n, shown.clone())), ["me@a", "me@b", "Local"]);
    names.update(cx, |n, cx| n.reorder(&shown, &b, &a, Side::Before, cx));
    assert_eq!(names.read_with(cx, |n, _| names_of(n, shown.clone())), ["me@b", "me@a", "Local"]);

    // Dropping on itself changes nothing.
    names.update(cx, |n, cx| n.reorder(&shown, &a, &a, Side::Before, cx));
    assert_eq!(names.read_with(cx, |n, _| names_of(n, shown.clone())), ["me@b", "me@a", "Local"]);

    // Saved, keyed by target: it survives a restart; a session connected later goes last
    // until it is dragged.
    let reloaded = SessionNames::new(Some(path.clone()));
    let with_c = vec![local.clone(), c.clone(), a.clone(), b.clone()];
    assert_eq!(names_of(&reloaded, with_c.clone()), ["me@b", "me@a", "Local", "me@c"]);
    names.update(cx, |n, cx| n.reorder(&with_c, &c, &b, Side::Before, cx));
    assert_eq!(names.read_with(cx, |n, _| names_of(n, with_c.clone())), ["me@c", "me@b", "me@a", "Local"]);

    // A disconnected session keeps its place among the others.
    names.update(cx, |n, cx| n.reorder(&shown, &local, &b, Side::Before, cx));
    assert_eq!(names.read_with(cx, |n, _| names_of(n, with_c.clone())), ["me@c", "Local", "me@b", "me@a"]);
}

#[gpui_kit::test]
fn recent_directories_are_kept_per_target_newest_first_up_to_ten(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ui-state.json");
    let names = cx.new(|_| SessionNames::new(Some(path.clone())));
    let local = ConnectionTarget::Local;
    assert!(names.read_with(cx, |n, _| n.recent_directories(&local).is_empty()));

    for index in 0..12 {
        names.update(cx, |n, cx| n.add_recent_directory(&local, &format!("/d{index}"), cx));
    }
    names.update(cx, |n, cx| n.add_recent_directory(&local, "/d5", cx));

    let expected = ["/d5", "/d11", "/d10", "/d9", "/d8", "/d7", "/d6", "/d4", "/d3", "/d2"];
    assert_eq!(names.read_with(cx, |n, _| n.recent_directories(&local).to_vec()), expected);
    assert_eq!(SessionNames::new(Some(path)).recent_directories(&local), expected, "saved to disk");
}
