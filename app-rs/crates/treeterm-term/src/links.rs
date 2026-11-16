//! Links in the terminal's text, found as Electron's two engines find them.
//!
//! - OSC 8 hyperlinks, as xterm.js's `OscLinkProvider` (Electron's terminal engine): only
//!   `http:` and `https:` URIs are links, since TreeTerm's `linkHandler` leaves
//!   `allowNonHttpProtocols` off; a `file:` or custom-scheme hyperlink is plain text, and so
//!   never launches an app. A link is the run of cells on its row with the same hyperlink.
//! - URLs in the text, as ghostty-web's `UrlRegexProvider` (Electron's Ghostty engine):
//!   `http(s)`, `mailto`, `ftp`, `ssh`, `git`, `tel`, `magnet`, `gemini`, `gopher` and `news`.
//!   `file://` is not among them.
//!
//! A link stays within its row, as xterm.js finds OSC 8 hyperlinks and both engines find
//! URLs in the text.

use std::ops::Range;
use std::sync::LazyLock;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use regex::Regex;

/// ghostty-web's `URL_REGEX`: a scheme, then URL characters (ASCII only, as `\w` is without
/// JavaScript's `u` flag).
static URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:https?://|mailto:|ftp://|ssh://|git://|tel:|magnet:|gemini://|gopher://|news:)[A-Za-z0-9_\-.~:/?#@!$&*+,;=%]+",
    )
    .expect("URL pattern compiles")
});

/// ghostty-web's `TRAILING_PUNCTUATION`, cut off a URL's end.
const TRAILING_PUNCTUATION: [char; 7] = ['.', ',', ';', '!', '?', ')', ']'];

/// ghostty-web keeps a URL only when it is longer than this.
const MIN_URL_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// An OSC 8 hyperlink: a click opens it, as in xterm.js.
    Hyperlink,
    /// A URL in the text: Cmd+click (macOS) or Ctrl+click (other OSes) opens it, so a plain
    /// click still selects the text under it.
    Url,
}

/// A link on one grid row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub uri: String,
    pub kind: LinkKind,
    pub line: Line,
    /// The cells it covers on `line`.
    pub columns: Range<usize>,
}

impl Link {
    /// Whether a click opens it, with Cmd (macOS) or Ctrl (other OSes) held or not. Only
    /// a link a click opens is underlined under the pointer.
    pub fn opens(&self, secondary: bool) -> bool {
        match self.kind {
            LinkKind::Hyperlink => true,
            LinkKind::Url => secondary,
        }
    }
}

/// The link covering `point`, if any: an OSC 8 `http(s)` hyperlink before a URL in the text.
pub fn link_at<T>(term: &Term<T>, point: Point) -> Option<Link> {
    let cols = term.columns();
    if point.line < term.topmost_line()
        || point.line > term.bottommost_line()
        || point.column.0 >= cols
    {
        return None;
    }
    let row = &term.grid()[point.line];
    let column = point.column.0;
    if let Some(hyperlink) = row[point.column].hyperlink()
        && is_web(hyperlink.uri())
    {
        // Equal hyperlinks have the same id and URI: xterm.js's `urlId`. One without an id
        // gets a fresh one each time it starts, as in xterm.js.
        let same = |column: usize| row[Column(column)].hyperlink().as_ref() == Some(&hyperlink);
        let mut start = column;
        while start > 0 && same(start - 1) {
            start -= 1;
        }
        let mut end = column + 1;
        while end < cols && same(end) {
            end += 1;
        }
        return Some(Link {
            uri: hyperlink.uri().to_owned(),
            kind: LinkKind::Hyperlink,
            line: point.line,
            columns: start..end,
        });
    }
    // One byte per cell, so a byte offset is a column. A URL is ASCII, so anything else
    // (wide characters and their spacers, controls, empty cells) ends one, as a space does.
    let text: String = (0..cols)
        .map(|column| {
            let c = row[Column(column)].c;
            if c.is_ascii_graphic() { c } else { ' ' }
        })
        .collect();
    URL.find_iter(&text).find_map(|found| {
        let url = found.as_str().trim_end_matches(TRAILING_PUNCTUATION);
        let columns = found.start()..found.start() + url.len();
        (url.len() > MIN_URL_LEN && columns.contains(&column)).then(|| Link {
            uri: url.to_owned(),
            kind: LinkKind::Url,
            line: point.line,
            columns,
        })
    })
}

/// Whether an OSC 8 URI is a link: it parses as an `http:` or `https:` URL (WHATWG, as
/// `new URL` reads it).
fn is_web(uri: &str) -> bool {
    url::Url::parse(uri).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CursorShapeSetting, EmulatorSettings, GridSize, TermCore};
    use crate::palette::{DEFAULT_BACKGROUND, Palette};

    /// A 60-column terminal showing `bytes`.
    fn core(bytes: &[u8]) -> TermCore {
        let mut core = TermCore::new(
            GridSize { cols: 60, rows: 4 },
            EmulatorSettings {
                cursor: CursorShapeSetting::Block,
                cursor_blink: false,
                allow_osc52_clipboard: false,
                scrollback: 100,
            },
            Palette::new(DEFAULT_BACKGROUND),
            None,
        );
        core.feed(bytes);
        core
    }

    fn at(core: &TermCore, line: i32, column: usize) -> Option<Link> {
        link_at(&*core.term().lock(), Point::new(Line(line), Column(column)))
    }

    /// The URI and columns of the link at each column of row 0 that has one.
    fn uri_and_columns(core: &TermCore, column: usize) -> Option<(String, Range<usize>)> {
        at(core, 0, column).map(|link| (link.uri, link.columns))
    }

    #[test]
    fn an_http_hyperlink_spans_exactly_its_cells() {
        let core = core(b"go \x1b]8;;https://example.com\x1b\\click\x1b]8;;\x1b\\ now");
        let link = Link {
            uri: "https://example.com".to_owned(),
            kind: LinkKind::Hyperlink,
            line: Line(0),
            columns: 3..8,
        };
        for column in 3..8 {
            assert_eq!(
                at(&core, 0, column).as_ref(),
                Some(&link),
                "column {column}"
            );
        }
        assert_eq!(at(&core, 0, 2), None);
        assert_eq!(at(&core, 0, 8), None);
    }

    #[test]
    fn a_hyperlink_to_anything_but_http_is_no_link() {
        for uri in [
            "file:///x",
            "ssh://host/repo",
            "vscode://open",
            "not a url",
            "https://",
        ] {
            let core = core(format!("\x1b]8;;{uri}\x1b\\click\x1b]8;;\x1b\\").as_bytes());
            assert_eq!(at(&core, 0, 1), None, "{uri}");
        }
        // Its text can still be a URL, which is then the link.
        let core = core(b"\x1b]8;;file:///x\x1b\\see https://a.example/b\x1b]8;;\x1b\\");
        let link = at(&core, 0, 6).expect("the URL in the text");
        assert_eq!(
            (link.kind, link.uri, link.columns),
            (LinkKind::Url, "https://a.example/b".to_owned(), 4..23)
        );
    }

    #[test]
    fn a_hyperlink_scheme_is_read_in_any_case() {
        let core = core(b"\x1b]8;;HTTP://Example.com/A\x1b\\click\x1b]8;;\x1b\\");
        assert_eq!(
            uri_and_columns(&core, 0),
            Some(("HTTP://Example.com/A".to_owned(), 0..5))
        );
    }

    #[test]
    fn adjacent_hyperlinks_are_separate_unless_they_share_an_id_and_uri() {
        // Different ids, and no ids (each start gets a fresh one), keep links apart.
        let core = core(
            b"\x1b]8;id=a;https://a.example\x1b\\ab\x1b]8;id=b;https://a.example\x1b\\cd\
              \x1b]8;;https://b.example\x1b\\ef\x1b]8;;https://b.example\x1b\\gh\x1b]8;;\x1b\\",
        );
        assert_eq!(
            uri_and_columns(&core, 1),
            Some(("https://a.example".to_owned(), 0..2))
        );
        assert_eq!(
            uri_and_columns(&core, 2),
            Some(("https://a.example".to_owned(), 2..4))
        );
        assert_eq!(
            uri_and_columns(&core, 4),
            Some(("https://b.example".to_owned(), 4..6))
        );
        assert_eq!(
            uri_and_columns(&core, 7),
            Some(("https://b.example".to_owned(), 6..8))
        );
        // The same id and URI started again continue one link, as xterm.js's `urlId` does.
        let core = self::core(
            b"\x1b]8;id=a;https://a.example\x1b\\ab\x1b]8;;\x1b\\\x1b]8;id=a;https://a.example\x1b\\cd\x1b]8;;\x1b\\",
        );
        assert_eq!(
            uri_and_columns(&core, 0),
            Some(("https://a.example".to_owned(), 0..4))
        );
    }

    #[test]
    fn a_hyperlink_covers_its_wide_characters_whole() {
        let core = core("a\x1b]8;;https://example.com\x1b\\日本\x1b]8;;\x1b\\b".as_bytes());
        // 日 and 本 take two cells each, their spacers included.
        assert_eq!(
            uri_and_columns(&core, 2),
            Some(("https://example.com".to_owned(), 1..5))
        );
        assert_eq!(uri_and_columns(&core, 5), None);
    }

    #[test]
    fn finds_the_url_under_a_column_without_trailing_punctuation() {
        let core = core(b"see https://example.com/a?b=1. for details");
        let link = at(&core, 0, 10).expect("a URL");
        assert_eq!(
            link,
            Link {
                uri: "https://example.com/a?b=1".to_owned(),
                kind: LinkKind::Url,
                line: Line(0),
                columns: 4..29,
            }
        );
        assert_eq!(at(&core, 0, 4).as_ref(), Some(&link));
        assert_eq!(at(&core, 0, 28).as_ref(), Some(&link));
        assert_eq!(at(&core, 0, 29), None, "the trimmed full stop");
        assert_eq!(at(&core, 0, 1), None);
    }

    #[test]
    fn links_ghostty_web_s_schemes_in_any_case() {
        for url in [
            "http://x.example",
            "HTTPS://X.EXAMPLE",
            "mailto:me@example.io",
            "ftp://host/file",
            "ssh://host/repo",
            "git://host/repo.git",
            "tel:+15551234567",
            "magnet:?xt=urn:btih:abc",
            "gemini://host/page",
            "gopher://host/1",
            "news:comp.lang.rust",
        ] {
            let core = core(format!("> {url} <").as_bytes());
            assert_eq!(
                uri_and_columns(&core, 3),
                Some((url.to_owned(), 2..2 + url.len())),
                "{url}"
            );
        }
    }

    #[test]
    fn a_url_must_be_longer_than_eight_characters() {
        assert_eq!(uri_and_columns(&core(b"news:abc"), 0), None);
        assert_eq!(
            uri_and_columns(&core(b"news:abcd"), 0),
            Some(("news:abcd".to_owned(), 0..9))
        );
        // Counted once its trailing punctuation is gone.
        assert_eq!(uri_and_columns(&core(b"news:abc!?"), 0), None);
    }

    #[test]
    fn files_and_unknown_schemes_are_no_urls() {
        for text in [
            "file:///etc/hosts",
            "vscode://file/x",
            "www.example.com/path",
        ] {
            assert_eq!(uri_and_columns(&core(text.as_bytes()), 3), None, "{text}");
        }
    }

    #[test]
    fn a_url_stops_at_brackets_quotes_and_spaces() {
        assert_eq!(
            uri_and_columns(&core(b"(http://x.io/y)"), 3),
            Some(("http://x.io/y".to_owned(), 1..14))
        );
        let core = core(b"https://en.wikipedia.org/wiki/Rust_(x)");
        assert_eq!(
            uri_and_columns(&core, 0),
            Some(("https://en.wikipedia.org/wiki/Rust_".to_owned(), 0..35))
        );
        assert_eq!(uri_and_columns(&core, 36), None);
        let core = self::core(b"'https://a.example/b' \"https://c.example\"");
        assert_eq!(
            uri_and_columns(&core, 3),
            Some(("https://a.example/b".to_owned(), 1..20))
        );
        assert_eq!(
            uri_and_columns(&core, 25),
            Some(("https://c.example".to_owned(), 23..40))
        );
    }

    #[test]
    fn a_url_starts_at_its_scheme_and_runs_on_through_punctuation_inside() {
        // ghostty-web matches the scheme anywhere, and `,` and `;` are URL characters.
        let core = core(b"xhttps://a.example,https://b.example;");
        assert_eq!(uri_and_columns(&core, 0), None);
        assert_eq!(
            uri_and_columns(&core, 30),
            Some(("https://a.example,https://b.example".to_owned(), 1..36))
        );
    }

    #[test]
    fn a_url_after_wide_characters_keeps_its_cells() {
        let core = core("日本 https://a.example".as_bytes());
        // 日 and 本 take columns 0..4; the space is column 4.
        assert_eq!(
            uri_and_columns(&core, 5),
            Some(("https://a.example".to_owned(), 5..22))
        );
        assert_eq!(uri_and_columns(&core, 4), None);
        // Non-ASCII ends a URL, as ghostty-web's ASCII character class does.
        let core = self::core("https://a.example/日本".as_bytes());
        assert_eq!(
            uri_and_columns(&core, 0),
            Some(("https://a.example/".to_owned(), 0..18))
        );
    }

    #[test]
    fn links_are_found_on_scrollback_rows_and_never_off_the_grid() {
        let core = core(b"https://old.example\r\n1\r\n2\r\n3\r\n4");
        assert_eq!(
            at(&core, -1, 3).map(|link| link.uri).as_deref(),
            Some("https://old.example")
        );
        assert_eq!(at(&core, -2, 0), None);
        assert_eq!(at(&core, 4, 0), None);
        assert_eq!(at(&core, 0, 60), None);
    }

    #[test]
    fn a_click_opens_a_hyperlink_and_a_url_only_under_cmd_or_ctrl() {
        let link = |kind| Link {
            uri: "https://example.com".to_owned(),
            kind,
            line: Line(0),
            columns: 0..5,
        };
        assert!(link(LinkKind::Hyperlink).opens(false));
        assert!(link(LinkKind::Hyperlink).opens(true));
        assert!(!link(LinkKind::Url).opens(false));
        assert!(link(LinkKind::Url).opens(true));
    }
}
