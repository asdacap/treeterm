//! Monaco's indentation detection (`detectIndentation`, on in every model Electron's editors
//! create): a file opens with Tab inserting what the file indents with (a tab in a
//! tab-indented file, else spaces as wide as its indent steps), and tabs drawn that wide.
//! A port of Monaco's `guessIndentation` (`indentationGuesser.ts`), which reads at most the
//! first 10,000 lines.

/// What Tab inserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentUnit {
    /// A tab character.
    Tab,
    /// [`Indentation::tab_size`] spaces.
    Spaces,
}

/// Monaco's `{ insertSpaces, tabSize }`: what Tab inserts, and how many columns a tab (and
/// an indent of spaces) spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indentation {
    pub unit: IndentUnit,
    pub tab_size: usize,
}

impl Indentation {
    /// Monaco's model defaults (`EDITOR_MODEL_DEFAULTS`): 4-space soft tabs.
    pub const MONACO_DEFAULT: Self = Self { unit: IndentUnit::Spaces, tab_size: 4 };
}

/// Lines Monaco reads.
const MAX_LINES: usize = 10_000;
/// Tab sizes Monaco may guess, even ones first.
const TAB_SIZE_GUESSES: [usize; 7] = [2, 4, 6, 8, 3, 5, 7];
/// The largest of [`TAB_SIZE_GUESSES`].
const MAX_TAB_SIZE_GUESS: usize = 8;

/// `spacesDiff`: how far a line's indentation steps from the previous line's.
#[derive(Debug, Default)]
struct Step {
    /// Spaces per step (0 when the two cannot be compared).
    spaces: usize,
    /// The step lines the line up under something on the previous line (a declaration
    /// continued after a comma), not an indent.
    looks_like_alignment: bool,
}

/// `spacesDiff` between the previous line with content `a` (its content starting at byte
/// `a_indent`) and `b`. Leading whitespace is tabs and spaces only, so byte offsets in it
/// are the UTF-16 offsets Monaco compares.
fn step(a: &str, a_indent: usize, b: &str, b_indent: usize) -> Step {
    let (a_bytes, b_bytes) = (a.as_bytes(), b.as_bytes());
    // The indentation the two lines share.
    let common = a_bytes[..a_indent].iter().zip(&b_bytes[..b_indent]).take_while(|(x, y)| x == y).count();
    let count = |rest: &[u8]| {
        let spaces = rest.iter().filter(|&&byte| byte == b' ').count();
        (spaces, rest.len() - spaces)
    };
    let (a_spaces, a_tabs) = count(&a_bytes[common..a_indent]);
    let (b_spaces, b_tabs) = count(&b_bytes[common..b_indent]);
    if (a_spaces > 0 && a_tabs > 0) || (b_spaces > 0 && b_tabs > 0) {
        return Step::default();
    }
    let tabs = a_tabs.abs_diff(b_tabs);
    let spaces = a_spaces.abs_diff(b_spaces);
    if tabs == 0 {
        // Indented to line up with the previous line, e.g.
        //   const a = b + c,
        //         d = b - c;
        let looks_like_alignment = spaces > 0
            && b_spaces >= 1
            && b_bytes.get(b_spaces).is_some_and(|&byte| byte != b' ')
            // Past the previous line's indentation the offset is in UTF-16 units.
            && a.encode_utf16().nth(b_spaces - 1) == Some(u16::from(b' '))
            && a.ends_with(',');
        return Step { spaces, looks_like_alignment };
    }
    if spaces % tabs == 0 {
        return Step { spaces: spaces / tabs, looks_like_alignment: false };
    }
    Step::default()
}

/// Monaco's lines: split at `\r\n`, `\r` or `\n`.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').flat_map(|line| line.strip_suffix('\r').unwrap_or(line).split('\r'))
}

/// `guessIndentation`: the indentation `text` uses, or `default` where it does not tell.
pub fn guess_indentation(text: &str, default: Indentation) -> Indentation {
    let default_spaces = default.unit == IndentUnit::Spaces;
    let mut tab_indented = 0usize;
    let mut space_indented = 0usize;
    // The last line with content that was not skipped, and where its content starts.
    let mut previous: (&str, usize) = ("", 0);
    // How often each step (in spaces) occurs between consecutive lines.
    let mut steps = [0usize; MAX_TAB_SIZE_GUESS + 1];
    for line in lines(text).take(MAX_LINES) {
        // Blank lines (only tabs and spaces) do not count.
        let Some(indent) = line.bytes().position(|byte| byte != b'\t' && byte != b' ') else {
            continue;
        };
        let tabs = line.as_bytes()[..indent].iter().filter(|&&byte| byte == b'\t').count();
        if tabs > 0 {
            tab_indented += 1;
        } else if indent > 1 {
            space_indented += 1;
        }
        let step = step(previous.0, previous.1, line, indent);
        // An alignment is skipped, unless it is one default indent (a nested list item).
        if step.looks_like_alignment && !(default_spaces && default.tab_size == step.spaces) {
            continue;
        }
        if let Some(count) = steps.get_mut(step.spaces) {
            *count += 1;
        }
        previous = (line, indent);
    }
    let spaces = if tab_indented == space_indented { default_spaces } else { tab_indented < space_indented };
    if !spaces {
        return Indentation { unit: IndentUnit::Tab, tab_size: default.tab_size };
    }
    let mut tab_size = default.tab_size;
    let mut score = 0;
    for guess in TAB_SIZE_GUESSES {
        if steps[guess] > score {
            score = steps[guess];
            tab_size = guess;
        }
    }
    // 2 wins over a guessed 4 when it occurs at least half as often.
    if tab_size == 4 && steps[4] > 0 && steps[2] > 0 && 2 * steps[2] >= steps[4] {
        tab_size = 2;
    }
    Indentation { unit: IndentUnit::Spaces, tab_size }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess(text: &str) -> Indentation {
        guess_indentation(text, Indentation::MONACO_DEFAULT)
    }

    fn spaces(tab_size: usize) -> Indentation {
        Indentation { unit: IndentUnit::Spaces, tab_size }
    }

    const TABS: Indentation = Indentation { unit: IndentUnit::Tab, tab_size: 4 };

    #[test]
    fn tab_indented_text_indents_with_tabs_drawn_four_wide() {
        assert_eq!(guess("all: build\n\tcargo build\n\ttouch done\n\nclean:\n\trm -f done\n"), TABS);
        assert_eq!(guess("func main() {\n\tif x {\n\t\treturn\n\t}\n}\n"), TABS);
    }

    #[test]
    fn space_indented_text_indents_by_its_step() {
        assert_eq!(guess("fn f() {\n  if x {\n    y();\n  }\n}\n"), spaces(2));
        assert_eq!(guess("def f():\n    if x:\n        return 1\n    return 2\n"), spaces(4));
        assert_eq!(guess("a:\n        b\nc:\n        d\n"), spaces(8));
        assert_eq!(guess("a\n   b\n      c\n   d\n"), spaces(3));
    }

    #[test]
    fn two_wins_over_four_when_half_as_common() {
        // Steps of 4 three times, of 2 twice: 2 >= 3 / 2.
        assert_eq!(guess("a\n    b\nc\n    d\ne\n    f\n  g\nh\n  i\n"), spaces(2));
        // Steps of 4 five times, of 2 twice: 4 < 5.
        assert_eq!(guess("a\n    b\nc\n    d\ne\n    f\n  g\nh\n    i\nj\n    k\nl\n  m\n"), spaces(4));
    }

    #[test]
    fn text_that_does_not_tell_keeps_the_default() {
        assert_eq!(guess(""), spaces(4));
        assert_eq!(guess("one\ntwo\nthree\n"), spaces(4));
        assert_eq!(guess("\n   \n\t\n"), spaces(4));
        // As many tab- as space-indented lines: the default unit, and a guessed width.
        assert_eq!(guess("a\n\tb\nc\n  d\n"), spaces(2));
        let tabs_default = Indentation { unit: IndentUnit::Tab, tab_size: 8 };
        assert_eq!(guess_indentation("a\n\tb\nc\n  d\n", tabs_default), tabs_default);
        // One space is not an indent.
        assert_eq!(guess("a\n b\nc\n d\n\te\n"), TABS);
    }

    #[test]
    fn mostly_tabs_wins_over_some_spaces() {
        assert_eq!(guess("a\n\tb\n\tc\n\td\n  e\n"), TABS);
        assert_eq!(guess("a\n  b\n  c\n  d\n\te\n"), spaces(2));
    }

    #[test]
    fn steps_compare_the_indentation_past_what_two_lines_share() {
        // Past a shared tab, four spaces.
        assert_eq!(step("\ta", 1, "\t    b", 5).spaces, 4);
        // Two tabs against eight spaces: four spaces a tab.
        assert_eq!(step("\t\tx", 2, "        y", 8).spaces, 4);
        assert_eq!(guess("\t\tx\n        y\n\t\tz\n        w\n"), spaces(4));
        // Three spaces against two tabs do not divide.
        assert_eq!(step("\t\tx", 2, "   y", 3).spaces, 0);
        // Tabs and spaces mixed past the shared part are not compared.
        assert_eq!(step("x", 0, "  \ty", 3).spaces, 0);
        assert_eq!(step("  \tx", 3, "y", 0).spaces, 0);
        assert_eq!(guess("\ta\n  \tb\n\t\tc\n"), TABS);
    }

    #[test]
    fn alignment_after_a_comma_is_not_an_indent() {
        // The continuation lines up 6 spaces in: an alignment, so 2 is guessed.
        let text = "fn f() {\n  let a = 1,\n        b = 2;\n  if x {\n    y();\n  }\n}\n";
        assert_eq!(guess(text), spaces(2));
        // Without the comma it is a step of 6.
        assert_eq!(guess("x\n      y\nz\n      w\n"), spaces(6));
        // An alignment one default indent wide counts as an indent.
        let text = "foo bar,\n    baz\nfoo bar,\n    baz\nx\n   y\n";
        assert_eq!(guess(text), spaces(4));
        assert_eq!(guess_indentation(text, spaces(2)), spaces(3));
        // The previous line's column is counted in UTF-16 units, as Monaco counts it.
        assert_eq!(guess("aé b,\n   c\n"), spaces(4));
        assert_eq!(guess("const éé = 1,\n       b = 2;\nx\n  y\n"), spaces(7));
    }

    #[test]
    fn line_endings_split_as_monaco_splits_them() {
        assert_eq!(guess("a\r\n\tb\r\n\tc\r\n"), TABS);
        assert_eq!(guess("a\r  b\r    c\r"), spaces(2));
    }

    #[test]
    fn only_the_first_ten_thousand_lines_are_read() {
        let mut text = "x\n".repeat(MAX_LINES - 1);
        text.push_str("y\n\tz\n\tw\n");
        assert_eq!(guess(&text), spaces(4));
    }
}
