//! The remote directory browser (`RemoteDirectoryBrowser.tsx`) against a real in-process
//! daemon: it lists only subdirectories, moves on a typed absolute path, and shows why a
//! directory could not be listed.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_model::dir_browser::{DirBrowser, DirBrowserEvent, Listing};

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
    browser.update(cx, |b, cx| b.go_up(cx));
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
