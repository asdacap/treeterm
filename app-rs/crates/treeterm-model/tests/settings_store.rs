//! SettingsStore: save on change, reload on external edits, propagate to live terminals.

use std::path::Path;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::settings::{self, Settings};
use treeterm_model::settings_store::{SettingsEvent, SettingsStore, SoundSave};

fn saved_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(settings::settings_path(dir)).unwrap()).unwrap()
}

/// A store over `dir` holding `settings`, with every `Changed` event counted.
fn store_with(
    cx: &mut TestAppContext,
    dir: &Path,
    settings: &Settings,
) -> (Entity<SettingsStore>, std::rc::Rc<std::cell::Cell<usize>>) {
    settings::save(dir, settings).unwrap();
    let path = dir.to_owned();
    let store = cx.new(|cx| SettingsStore::new(Some(path), cx));
    let changes = std::rc::Rc::new(std::cell::Cell::new(0));
    let counted = changes.clone();
    cx.update(|cx| {
        cx.subscribe(&store, move |_, event: &SettingsEvent, _| match event {
            SettingsEvent::Changed => counted.set(counted.get() + 1),
        })
        .detach();
    });
    (store, changes)
}

/// settings.json becomes a directory, so every write to it fails (even as root).
fn block_writes(dir: &Path) {
    std::fs::remove_file(settings::settings_path(dir)).unwrap();
    std::fs::create_dir(settings::settings_path(dir)).unwrap();
}

fn unblock_writes(dir: &Path) {
    std::fs::remove_dir(settings::settings_path(dir)).unwrap();
}

fn save(
    cx: &mut TestAppContext,
    store: &Entity<SettingsStore>,
    change: impl FnOnce(&mut Settings) + 'static,
) -> futures::channel::oneshot::Receiver<Result<(), String>> {
    store.update(cx, |s, cx| s.save(change, cx))
}

fn outcome(cx: &mut TestAppContext, saved: futures::channel::oneshot::Receiver<Result<(), String>>) -> Result<(), String> {
    cx.run_until_parked();
    futures::executor::block_on(saved).expect("the store replies")
}

#[gpui_kit::test]
fn changes_are_saved_and_external_edits_reload(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_owned();
    let store = cx.new(|cx| SettingsStore::new(Some(path.clone()), cx));
    store.read_with(cx, |s, _| assert_eq!(s.settings().terminal.max_cols, 160));

    store.update(cx, |s, cx| s.update(|settings| settings.terminal.max_cols = 120, cx));
    cx.run_until_parked();
    let saved = saved_json(&path);
    assert_eq!(saved["terminal"]["maxCols"], 120);
    store.read_with(cx, |s, _| assert_eq!(s.settings().terminal.max_cols, 120));

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
    // Only a save replaces it.
    assert_eq!(std::fs::read(dir.path().join("settings.json")).unwrap(), b"{nope");
}

/// `dialog:selectFolder`: a folder picked in the native picker goes to the front of the
/// saved recent directories, once, keeping ten.
#[gpui_kit::test]
fn a_picked_folder_goes_to_the_front_of_the_saved_recent_directories(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mine =
        Settings { recent_directories: (0..10).map(|index| format!("/d{index}")).collect(), ..Settings::default() };
    let (store, _) = store_with(cx, dir.path(), &mine);

    store.update(cx, |s, cx| s.add_recent_directory("/d4", cx));
    store.update(cx, |s, cx| s.add_recent_directory("/new", cx));
    cx.run_until_parked();
    let expected = ["/new", "/d4", "/d0", "/d1", "/d2", "/d3", "/d5", "/d6", "/d7", "/d8"];
    assert_eq!(saved_json(dir.path())["recentDirectories"], serde_json::json!(expected));
    assert_eq!(settings::load(dir.path()).unwrap().recent_directories, expected);
    store.read_with(cx, |s, _| assert_eq!(s.settings().recent_directories, expected));
}

/// A pick never writes the settings held in place of a `settings.json` that failed to load.
#[gpui_kit::test]
fn a_picked_folder_leaves_a_corrupt_settings_file_alone(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(settings::settings_path(dir.path()), b"{nope").unwrap();
    let path = dir.path().to_owned();
    let store = cx.new(|cx| SettingsStore::new(Some(path), cx));

    store.update(cx, |s, cx| s.add_recent_directory("/picked", cx));
    cx.run_until_parked();
    assert_eq!(std::fs::read(settings::settings_path(dir.path())).unwrap(), b"{nope");
    store.read_with(cx, |s, _| {
        assert!(s.settings().recent_directories.is_empty());
        assert!(s.error().is_some());
    });
}

/// Electron's `loadSettings` writes the defaults when there is no file.
#[gpui_kit::test]
fn a_missing_settings_file_is_created_with_the_defaults(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("userData");
    let path = dir.clone();
    let store = cx.new(|cx| SettingsStore::new(Some(path), cx));
    let written = std::fs::read(settings::settings_path(&dir)).expect("settings.json is created");
    assert_eq!(written, serde_json::to_vec_pretty(&Settings::default()).unwrap());
    store.read_with(cx, |s, _| assert_eq!(s.error(), None));

    // An existing file is left as it is.
    let mut mine = Settings::default();
    mine.terminal.max_cols = 90;
    settings::save(&dir, &mine).unwrap();
    let before = std::fs::read(settings::settings_path(&dir)).unwrap();
    let path = dir.clone();
    let store = cx.new(|cx| SettingsStore::new(Some(path), cx));
    assert_eq!(std::fs::read(settings::settings_path(&dir)).unwrap(), before);
    store.read_with(cx, |s, _| assert_eq!(s.settings().terminal.max_cols, 90));
}

#[gpui_kit::test]
fn a_settings_file_that_cannot_be_created_is_reported(cx: &mut TestAppContext) {
    // The user-data "directory" is a file: neither it nor settings.json can be created.
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_owned();
    let store = cx.new(|cx| SettingsStore::new(Some(path), cx));
    store.read_with(cx, |s, _| {
        assert!(s.error().unwrap().starts_with("cannot write"), "{:?}", s.error());
        assert_eq!(**s.settings(), Settings::default());
    });
}

/// `saveSettings` sets the settings only once written: a failed save changes nothing, and
/// saving the same change again retries the write.
#[gpui_kit::test]
fn a_failed_save_keeps_the_settings_and_the_next_save_retries(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (store, changes) = store_with(cx, dir.path(), &Settings::default());

    block_writes(dir.path());
    let saved = save(cx, &store, |s| s.terminal.max_cols = 120);
    let error = outcome(cx, saved).unwrap_err();
    assert!(error.starts_with("cannot write"), "{error}");
    store.read_with(cx, |s, _| {
        assert_eq!(s.settings().terminal.max_cols, 160);
        assert_eq!(s.error(), Some(error.as_str()));
    });
    assert_eq!(changes.get(), 0);

    unblock_writes(dir.path());
    let saved = save(cx, &store, |s| s.terminal.max_cols = 120);
    assert_eq!(outcome(cx, saved), Ok(()));
    assert_eq!(saved_json(dir.path())["terminal"]["maxCols"], 120);
    store.read_with(cx, |s, _| {
        assert_eq!(s.settings().terminal.max_cols, 120);
        assert_eq!(s.error(), None);
    });
    assert_eq!(changes.get(), 1);
}

/// Electron writes the file on every save, even when nothing changed: Save with no edits
/// replaces a corrupt file and succeeds.
#[gpui_kit::test]
fn an_unchanged_save_writes_and_clears_a_load_error(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(settings::settings_path(dir.path()), b"{nope").unwrap();
    let path = dir.path().to_owned();
    let store = cx.new(|cx| SettingsStore::new(Some(path), cx));
    store.read_with(cx, |s, _| assert!(s.error().is_some()));

    let saved = save(cx, &store, |_| {});
    assert_eq!(outcome(cx, saved), Ok(()));
    assert_eq!(settings::load(dir.path()).unwrap(), Settings::default());
    store.read_with(cx, |s, _| assert_eq!(s.error(), None));
}

/// Saves are written one at a time in call order, each applied to the settings the earlier
/// ones left.
#[gpui_kit::test]
fn saves_run_in_call_order_each_seeing_the_previous(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (store, changes) = store_with(cx, dir.path(), &Settings::default());

    let first = save(cx, &store, |s| s.terminal.max_cols = 100);
    let second = save(cx, &store, |s| s.terminal.max_cols += 1);
    store.update(cx, |s, cx| s.update(|s| s.terminal.max_cols *= 2, cx));
    assert_eq!(outcome(cx, first), Ok(()));
    assert_eq!(outcome(cx, second), Ok(()));
    store.read_with(cx, |s, _| assert_eq!(s.settings().terminal.max_cols, 202));
    assert_eq!(saved_json(dir.path())["terminal"]["maxCols"], 202);
    assert_eq!(changes.get(), 3);
}

/// A fire-and-forget update that fails is recorded and changes nothing.
#[gpui_kit::test]
fn a_failed_update_is_recorded(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (store, changes) = store_with(cx, dir.path(), &Settings::default());

    block_writes(dir.path());
    store.update(cx, |s, cx| s.update(|s| s.terminal.max_cols = 120, cx));
    cx.run_until_parked();
    store.read_with(cx, |s, _| {
        assert_eq!(s.settings().terminal.max_cols, 160);
        assert!(s.error().unwrap().starts_with("cannot write"), "{:?}", s.error());
    });
    assert_eq!(changes.get(), 0);
}

/// `toggleSound`: Saving while the write runs (further toggles ignored), then Idle with the
/// sound flipped; a failed save is Failed with why and keeps the sound as it was.
#[gpui_kit::test]
fn toggle_sound_goes_saving_then_idle_or_failed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store_with(cx, dir.path(), &Settings::default());
    let sound = |cx: &mut TestAppContext| {
        store.read_with(cx, |s, _| (s.sound_save().clone(), s.settings().notifications.sound_enabled))
    };

    store.update(cx, |s, cx| s.toggle_sound(cx));
    assert_eq!(sound(cx), (SoundSave::Saving, true));
    store.update(cx, |s, cx| s.toggle_sound(cx)); // ignored while saving
    cx.run_until_parked();
    assert_eq!(sound(cx), (SoundSave::Idle, false));
    assert_eq!(saved_json(dir.path())["notifications"]["soundEnabled"], false);

    block_writes(dir.path());
    store.update(cx, |s, cx| s.toggle_sound(cx));
    cx.run_until_parked();
    let (state, enabled) = sound(cx);
    assert!(matches!(&state, SoundSave::Failed(error) if error.starts_with("cannot write")), "{state:?}");
    assert!(!enabled);

    // The next toggle tries again.
    unblock_writes(dir.path());
    store.update(cx, |s, cx| s.toggle_sound(cx));
    assert_eq!(sound(cx), (SoundSave::Saving, false));
    cx.run_until_parked();
    assert_eq!(sound(cx), (SoundSave::Idle, true));
    assert_eq!(saved_json(dir.path())["notifications"]["soundEnabled"], true);
}

/// Without a user data dir there is nothing to write: saves succeed at once.
#[gpui_kit::test]
fn an_in_memory_store_applies_saves_at_once(cx: &mut TestAppContext) {
    let store = cx.new(|_| SettingsStore::in_memory(Settings::default()));

    let saved = save(cx, &store, |s| s.terminal.max_cols = 120);
    store.read_with(cx, |s, _| assert_eq!(s.settings().terminal.max_cols, 120));
    assert_eq!(outcome(cx, saved), Ok(()));
    store.update(cx, |s, cx| s.update(|s| s.terminal.max_cols = 90, cx));
    store.read_with(cx, |s, _| assert_eq!(s.settings().terminal.max_cols, 90));

    store.update(cx, |s, cx| s.toggle_sound(cx));
    cx.run_until_parked();
    store.read_with(cx, |s, _| {
        assert_eq!(*s.sound_save(), SoundSave::Idle);
        assert!(!s.settings().notifications.sound_enabled);
    });
}

/// `ssh:saveConnection`: connecting again to a saved `user@host:port` from the picker's form
/// (a new id each time) replaces that entry where it is; another port is a new entry.
#[gpui_kit::test]
fn a_saved_ssh_connection_replaces_the_one_for_the_same_destination(cx: &mut TestAppContext) {
    use treeterm_core::settings::PortForwardSpec;
    use treeterm_model::ssh_connection_form::{ConnectionFields, build_config};

    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store_with(cx, dir.path(), &Settings::default());
    let form = |host: &str, port: &str, forwards: Vec<PortForwardSpec>, now_ms: u128| {
        let fields =
            ConnectionFields { host: host.into(), user: "me".into(), port: port.into(), ..ConnectionFields::default() };
        build_config(&fields, forwards, now_ms).unwrap()
    };
    let forward =
        PortForwardSpec { local_port: 8080, remote_host: "localhost".into(), remote_port: 3000, extra: Default::default() };

    store.update(cx, |s, cx| s.save_ssh_connection(form("box", "22", vec![], 1), cx));
    store.update(cx, |s, cx| s.save_ssh_connection(form("other", "22", vec![], 2), cx));
    let again = form("box", "22", vec![forward], 3);
    store.update(cx, |s, cx| s.save_ssh_connection(again.clone(), cx));
    store.update(cx, |s, cx| s.save_ssh_connection(form("box", "2222", vec![], 4), cx));
    cx.run_until_parked();

    let saved = settings::load(dir.path()).unwrap().ssh.saved_connections;
    let ids: Vec<&str> = saved.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["ssh-box-3", "ssh-other-2", "ssh-box-4"]);
    assert_eq!(saved[0], again, "the newest config, forwards included");
    assert_eq!(saved_json(dir.path())["ssh"]["savedConnections"][0]["label"], "me@box");
    store.read_with(cx, |s, _| assert_eq!(s.settings().ssh.saved_connections, saved));
}
