//! Custom session names persist in `ui-state.json` and fall back to the default name.

use gpui_kit::{AppContext as _, TestAppContext};
use treeterm_core::connection::ConnectionTarget;
use treeterm_model::session_names::{SessionNames, Side};

#[gpui_kit::test]
fn a_rename_persists_and_a_blank_name_restores_the_default(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ui-state.json");
    let names = cx.new(|_| SessionNames::new(Some(dir.path().to_owned())));
    let local = ConnectionTarget::Local;
    // `deriveDefaultSessionName`: the rename field starts with it.
    assert_eq!(names.read_with(cx, |n, _| n.name(&local)), "LOCAL");

    names.update(cx, |n, cx| n.rename(&local, "  Laptop ", cx));
    assert_eq!(names.read_with(cx, |n, _| n.name(&local)), "Laptop");
    let reloaded = SessionNames::new(Some(dir.path().to_owned()));
    assert_eq!(reloaded.name(&local), "Laptop", "saved to disk");

    names.update(cx, |n, cx| n.rename(&local, "   ", cx));
    assert_eq!(SessionNames::new(Some(dir.path().to_owned())).name(&local), "LOCAL");

    names.update(cx, |n, cx| n.rename(&local, "Laptop", cx));
    names.update(cx, |n, cx| n.rename(&local, " LOCAL ", cx));
    assert_eq!(names.read_with(cx, |n, _| n.name(&local)), "LOCAL");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("sessionNames"), "the default name is not stored: {text}");
}

#[gpui_kit::test]
fn only_the_exact_default_name_clears_a_custom_name(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let names = cx.new(|_| SessionNames::new(Some(dir.path().to_owned())));
    let (local, host) = (ConnectionTarget::Local, remote("a"));

    // `handleSaveName` compares with the default exactly: "Local" is a name of its own.
    names.update(cx, |n, cx| n.rename(&local, "Local", cx));
    assert_eq!(SessionNames::new(Some(dir.path().to_owned())).name(&local), "Local", "saved to disk");

    // A remote's default is its label (`user@host` here).
    names.update(cx, |n, cx| n.rename(&host, "Build box", cx));
    assert_eq!(names.read_with(cx, |n, _| n.name(&host)), "Build box");
    names.update(cx, |n, cx| n.rename(&host, "me@a", cx));
    assert_eq!(SessionNames::new(Some(dir.path().to_owned())).name(&host), "me@a");
    assert_eq!(names.read_with(cx, |n, _| n.name(&local)), "Local", "the other session keeps its name");
}

#[gpui_kit::test]
fn a_corrupt_file_is_moved_aside_and_saving_carries_on(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ui-state.json");
    std::fs::write(&path, "{broken").unwrap();
    let names = cx.new(|_| SessionNames::new(Some(dir.path().to_owned())));
    let local = ConnectionTarget::Local;
    assert_eq!(names.read_with(cx, |n, _| n.name(&local)), "LOCAL", "starts empty");

    names.update(cx, |n, cx| n.rename(&local, "Laptop", cx));
    assert_eq!(SessionNames::new(Some(dir.path().to_owned())).name(&local), "Laptop", "saved to disk");
    let backups: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.starts_with("ui-state.json.corrupt-"))
        .collect();
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert_eq!(std::fs::read_to_string(dir.path().join(&backups[0])).unwrap(), "{broken", "kept");
}

#[gpui_kit::test]
fn an_unreadable_file_is_left_alone(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ui-state.json");
    std::fs::create_dir(&path).unwrap();
    // Not even Electron's recent directories are carried over: saving would replace the file.
    electron_recent_directories(dir.path(), &serde_json::json!({ "local": ["/electron"] }));
    let names = cx.new(|_| SessionNames::new(Some(dir.path().to_owned())));
    let local = ConnectionTarget::Local;
    assert!(names.read_with(cx, |n, _| n.recent_directories(&local).is_empty()));

    names.update(cx, |n, cx| n.rename(&local, "Laptop", cx));
    assert_eq!(names.read_with(cx, |n, _| n.name(&local)), "Laptop", "for this run");
    assert!(path.is_dir() && std::fs::read_dir(&path).unwrap().next().is_none(), "not written");
}

/// Electron's localStorage in `user_data_dir` holding `directories` (by Electron's connection
/// key) as the Open Workspace dialog's Recent Directories.
fn electron_recent_directories(user_data_dir: &std::path::Path, directories: &serde_json::Value) {
    use treeterm_core::electron_local_storage::testing::{item_key, item_value, write_db};
    use treeterm_core::electron_local_storage::{ELECTRON_ORIGIN, leveldb_dir};
    let key = item_key(ELECTRON_ORIGIN, "treeterm-recent-directories");
    let value = serde_json::json!({ "state": { "directories": directories }, "version": 0 }).to_string();
    let dir = leveldb_dir(user_data_dir);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    write_db(&dir, &[(&key, &item_value(&value))], &[]);
}

#[gpui_kit::test]
fn electron_recent_directories_are_carried_over_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    electron_recent_directories(
        dir.path(),
        &serde_json::json!({ "local": ["/work/one", "/work/two"], "me@a:22": ["/home/me/repo"] }),
    );
    let names = cx.new(|_| SessionNames::new(Some(dir.path().to_owned())));
    let (local, host) = (ConnectionTarget::Local, remote("a"));
    assert_eq!(names.read_with(cx, |n, _| n.recent_directories(&local).to_vec()), ["/work/one", "/work/two"]);
    assert_eq!(names.read_with(cx, |n, _| n.recent_directories(&host).to_vec()), ["/home/me/repo"]);
    assert_eq!(
        SessionNames::new(Some(dir.path().to_owned())).recent_directories(&local),
        ["/work/one", "/work/two"],
        "saved to disk"
    );

    // Electron's later changes are not carried over: each app keeps its own lists.
    names.update(cx, |n, cx| n.add_recent_directory(&local, "/work/native", cx));
    electron_recent_directories(dir.path(), &serde_json::json!({ "local": ["/work/later"] }));
    assert_eq!(
        SessionNames::new(Some(dir.path().to_owned())).recent_directories(&local),
        ["/work/native", "/work/one", "/work/two"]
    );
}

#[gpui_kit::test]
fn electron_recent_directories_that_cannot_be_read_are_tried_again_at_the_next_start(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ui-state.json");
    let local = ConnectionTarget::Local;
    // No Electron localStorage, then one that cannot be opened: nothing to save.
    for _ in 0..2 {
        let names = cx.new(|_| SessionNames::new(Some(dir.path().to_owned())));
        assert!(names.read_with(cx, |n, _| n.recent_directories(&local).is_empty()));
        assert!(!path.exists());
        std::fs::create_dir_all(dir.path().join("Local Storage").join("leveldb")).unwrap();
    }

    electron_recent_directories(dir.path(), &serde_json::json!({ "local": ["/work/one"] }));
    assert_eq!(SessionNames::new(Some(dir.path().to_owned())).recent_directories(&local), ["/work/one"]);
    assert!(path.exists());
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
    let names = cx.new(|_| SessionNames::new(Some(dir.path().to_owned())));
    let (local, a, b, c) = (ConnectionTarget::Local, remote("a"), remote("b"), remote("c"));
    let shown = vec![local.clone(), a.clone(), b.clone()];
    // Never dragged: the order given.
    assert_eq!(names.read_with(cx, |n, _| names_of(n, shown.clone())), ["LOCAL", "me@a", "me@b"]);

    // An unnamed session never dragged before can be dragged, after or before another.
    names.update(cx, |n, cx| n.reorder(&shown, &local, &b, Side::After, cx));
    assert_eq!(names.read_with(cx, |n, _| names_of(n, shown.clone())), ["me@a", "me@b", "LOCAL"]);
    names.update(cx, |n, cx| n.reorder(&shown, &b, &a, Side::Before, cx));
    assert_eq!(names.read_with(cx, |n, _| names_of(n, shown.clone())), ["me@b", "me@a", "LOCAL"]);

    // Dropping on itself changes nothing.
    names.update(cx, |n, cx| n.reorder(&shown, &a, &a, Side::Before, cx));
    assert_eq!(names.read_with(cx, |n, _| names_of(n, shown.clone())), ["me@b", "me@a", "LOCAL"]);

    // Saved, keyed by target: it survives a restart; a session connected later goes last
    // until it is dragged.
    let reloaded = SessionNames::new(Some(dir.path().to_owned()));
    let with_c = vec![local.clone(), c.clone(), a.clone(), b.clone()];
    assert_eq!(names_of(&reloaded, with_c.clone()), ["me@b", "me@a", "LOCAL", "me@c"]);
    names.update(cx, |n, cx| n.reorder(&with_c, &c, &b, Side::Before, cx));
    assert_eq!(names.read_with(cx, |n, _| names_of(n, with_c.clone())), ["me@c", "me@b", "me@a", "LOCAL"]);

    // A disconnected session keeps its place among the others.
    names.update(cx, |n, cx| n.reorder(&shown, &local, &b, Side::Before, cx));
    assert_eq!(names.read_with(cx, |n, _| names_of(n, with_c.clone())), ["me@c", "LOCAL", "me@b", "me@a"]);
}

#[gpui_kit::test]
fn recent_directories_are_kept_per_target_newest_first_up_to_ten(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let names = cx.new(|_| SessionNames::new(Some(dir.path().to_owned())));
    let local = ConnectionTarget::Local;
    assert!(names.read_with(cx, |n, _| n.recent_directories(&local).is_empty()));

    for index in 0..12 {
        names.update(cx, |n, cx| n.add_recent_directory(&local, &format!("/d{index}"), cx));
    }
    names.update(cx, |n, cx| n.add_recent_directory(&local, "/d5", cx));

    let expected = ["/d5", "/d11", "/d10", "/d9", "/d8", "/d7", "/d6", "/d4", "/d3", "/d2"];
    assert_eq!(names.read_with(cx, |n, _| n.recent_directories(&local).to_vec()), expected);
    assert_eq!(SessionNames::new(Some(dir.path().to_owned())).recent_directories(&local), expected, "saved to disk");
}
