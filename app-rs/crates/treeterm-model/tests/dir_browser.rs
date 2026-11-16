//! The remote directory browser (`RemoteDirectoryBrowser.tsx`) against a real in-process
//! daemon: it lists only subdirectories, moves on a typed absolute path, and shows why a
//! directory could not be listed.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_model::dir_browser::{Commit, DirBrowser, DirBrowserEvent, Listing};

mod common;
use common::{harness, wait_until};

/// The listing's directories as (name, path).
fn listed(browser: &DirBrowser) -> Option<Vec<(String, String)>> {
    match browser.listing() {
        Listing::Loaded(entries) => Some(
            entries
                .iter()
                .map(|e| (e.name.clone(), e.path.clone()))
                .collect(),
        ),
        Listing::Loading | Listing::Failed(_) => None,
    }
}

fn loaded(browser: &DirBrowser) -> bool {
    !matches!(browser.listing(), Listing::Loading)
}

#[gpui_kit::test]
fn the_browser_lists_subdirectories_and_follows_typed_absolute_paths(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let root = h.daemon.root().join("browse");
    for dir in ["a", "b/inner", ".hidden"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("f.txt"), "not a directory").unwrap();
    let root = root.to_string_lossy().into_owned();

    let browser: Entity<DirBrowser> =
        cx.new(|cx| DirBrowser::new(h.conn.fs_client(), root.clone(), cx));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _events = cx.update(|cx| {
        cx.subscribe(&browser, move |_, event: &DirBrowserEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
    });

    // "Loading..." until the daemon answers; then the directories only (no files, no
    // hidden entries), in the daemon's order, with their absolute paths.
    assert_eq!(
        browser.read_with(cx, |b, _| b.listing().clone()),
        Listing::Loading
    );
    assert_eq!(browser.read_with(cx, |b, _| b.current().to_owned()), root);
    wait_until(cx, &browser, loaded);
    assert_eq!(
        browser.read_with(cx, |b, _| listed(b)),
        Some(vec![
            ("a".to_owned(), format!("{root}/a")),
            ("b".to_owned(), format!("{root}/b"))
        ])
    );

    // Into a directory: the path field is told where the browser went.
    browser.update(cx, |b, cx| b.navigate(format!("{root}/b"), cx));
    assert_eq!(
        browser.read_with(cx, |b, _| b.listing().clone()),
        Listing::Loading
    );
    wait_until(cx, &browser, loaded);
    assert_eq!(
        browser.read_with(cx, |b, _| listed(b)),
        Some(vec![("inner".to_owned(), format!("{root}/b/inner"))])
    );
    assert_eq!(
        *seen.borrow(),
        vec![DirBrowserEvent::Navigated(format!("{root}/b"))]
    );

    // A relative path is ignored; an absolute one is trimmed and listed.
    browser.update(cx, |b, cx| b.submit("relative/path", cx));
    browser.update(cx, |b, cx| b.submit("   ", cx));
    assert_eq!(
        browser.read_with(cx, |b, _| b.current().to_owned()),
        format!("{root}/b")
    );
    browser.update(cx, |b, cx| b.submit(&format!("  {root}/a "), cx));
    assert_eq!(
        browser.read_with(cx, |b, _| b.current().to_owned()),
        format!("{root}/a")
    );
    wait_until(cx, &browser, loaded);
    assert_eq!(
        browser.read_with(cx, |b, _| listed(b)),
        Some(Vec::new()),
        "\"No subdirectories\""
    );

    // ".." goes back to the parent.
    browser.update(cx, |b, cx| b.go_up(&format!("{root}/a"), cx));
    assert_eq!(browser.read_with(cx, |b, _| b.current().to_owned()), root);
    assert!(!browser.read_with(cx, |b, _| b.at_root()));
    wait_until(cx, &browser, loaded);
    assert_eq!(
        browser.read_with(cx, |b, _| listed(b).map(|dirs| dirs.len())),
        Some(2)
    );
    assert_eq!(
        *seen.borrow(),
        vec![
            DirBrowserEvent::Navigated(format!("{root}/b")),
            DirBrowserEvent::Navigated(format!("{root}/a")),
            DirBrowserEvent::Navigated(root.clone()),
        ]
    );

    // A directory that cannot be listed shows the daemon's reason.
    browser.update(cx, |b, cx| b.navigate(format!("{root}/missing"), cx));
    wait_until(cx, &browser, loaded);
    match browser.read_with(cx, |b, _| b.listing().clone()) {
        Listing::Failed(reason) => assert!(!reason.is_empty()),
        other => panic!("expected a failure, got {other:?}"),
    }

    // The root has no parent.
    browser.update(cx, |b, cx| b.navigate("/".to_owned(), cx));
    assert!(browser.read_with(cx, |b, _| b.at_root()));
    assert_eq!(browser.read_with(cx, |b, _| b.crumbs().len()), 1);
}

/// The path field commits a typed path when it loses focus (`onBlur`), which it does as one
/// of the browser's buttons is pressed: Select, "..", a breadcrumb or an entry then acts on
/// the typed path, not on the directory shown before.
#[gpui_kit::test]
fn a_typed_path_is_committed_before_the_browsers_buttons_act(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let root = h.daemon.root().join("commit");
    for dir in ["a/inner", "b/deep/er"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    let root = root.to_string_lossy().into_owned();
    let (a, b) = (format!("{root}/a"), format!("{root}/b"));

    let browser: Entity<DirBrowser> =
        cx.new(|cx| DirBrowser::new(h.conn.fs_client(), root.clone(), cx));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _events = cx.update(|cx| {
        cx.subscribe(&browser, move |_, event: &DirBrowserEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
    });
    wait_until(cx, &browser, loaded);
    let current = |cx: &mut TestAppContext| browser.read_with(cx, |b, _| b.current().to_owned());

    // The directory shown (even with spaces), a relative path or nothing: no move, and
    // unlike Enter, no new read.
    for typed in [
        root.clone(),
        format!(" {root} "),
        "a".to_owned(),
        String::new(),
    ] {
        let commit = browser.update(cx, |b, cx| b.commit(&typed, cx));
        assert_eq!(commit, Commit::Stayed, "{typed:?}");
    }
    assert!(seen.borrow().is_empty());
    assert!(browser.read_with(cx, |b, _| loaded(b)));

    // Another absolute path: the browser goes there, trimmed.
    let commit = browser.update(cx, |b, cx| b.commit(&format!("  {a} "), cx));
    assert_eq!(commit, Commit::Moved);
    assert_eq!(current(cx), a);
    assert_eq!(*seen.borrow(), vec![DirBrowserEvent::Navigated(a.clone())]);

    // Select takes the typed directory, else the one shown.
    assert_eq!(browser.update(cx, |br, cx| br.select(&b, cx)), b);
    assert_eq!(current(cx), b);
    assert_eq!(browser.update(cx, |br, cx| br.select("relative", cx)), b);

    // ".." goes up from the typed path. A typed root stays there: ".." is disabled.
    browser.update(cx, |br, cx| br.go_up(&format!("{b}/deep/er"), cx));
    assert_eq!(current(cx), format!("{b}/deep"));
    browser.update(cx, |br, cx| br.go_up("/", cx));
    assert_eq!(current(cx), "/");
    let moves = seen.borrow().len();
    browser.update(cx, |br, cx| br.go_up("/", cx));
    assert_eq!(seen.borrow().len(), moves, "\"..\" is disabled at the root");

    // An entry of the listing shown before a typed path went with it; with nothing new
    // typed, the entry opens.
    browser.update(cx, |br, cx| br.navigate(root.clone(), cx));
    browser.update(cx, |br, cx| br.open_entry(&b, a.clone(), cx));
    assert_eq!(current(cx), b);
    browser.update(cx, |br, cx| br.open_entry(&b, format!("{b}/deep"), cx));
    assert_eq!(current(cx), format!("{b}/deep"));

    // A breadcrumb lands when it is still a link of the typed path's breadcrumbs (an
    // ancestor); with nothing new typed, it leads where it says.
    browser.update(cx, |br, cx| {
        br.open_crumb(&format!("{b}/deep/er"), b.clone(), cx)
    });
    assert_eq!(current(cx), b);
    browser.update(cx, |br, cx| br.open_crumb(&a, format!("{b}/deep"), cx));
    assert_eq!(current(cx), a, "not a breadcrumb of {a}");
    browser.update(cx, |br, cx| {
        br.open_crumb(&format!("{a}/inner"), a.clone(), cx)
    });
    assert_eq!(current(cx), a, "a breadcrumb of {a}/inner");
    browser.update(cx, |br, cx| br.open_crumb(&a, "/".to_owned(), cx));
    assert_eq!(current(cx), "/");
    wait_until(cx, &browser, loaded);
}
