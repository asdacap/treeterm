//! Monaco's `fuzzyScore` (`vs/base/common/filters.ts`), which its suggest widget filters and
//! ranks every suggestion with: `fuzzyScoreGracefulAggressive` (`editor.suggest.filterGraceful`)
//! with the default options, `boostFullMatch` and `matchOnWordStartOnly`.
//!
//! The typed characters must occur in the word in order, in any case, and the first one at
//! a strong place: the word's start, a capital after a lower-case letter (camelCase), a
//! separator, or the character after one (`bar` and `fb` match `fooBar`; `oo` does not
//! match `foobar`). Swapping two neighbouring typed characters but the first is tried too,
//! as a typo, at a small cost (`cno` matches `console`).
//!
//! Positions are characters; Monaco's are UTF-16 code units, which differ only past the
//! Basic Multilingual Plane.

/// Monaco compares at most this many characters of a pattern and of a word.
const MAX_LEN: usize = 128;

/// How `pattern` matched a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Higher is better.
    pub score: i32,
    /// The word's matched characters (indices), in order.
    pub positions: Vec<usize>,
}

/// How `pattern` (typed, not empty) matches `word`; `None` when it does not.
pub fn score(pattern: &str, word: &str) -> Option<Match> {
    let pattern: Vec<char> = pattern.chars().collect();
    let word: Vec<char> = word.chars().collect();
    let word_low = lower(&word);
    let mut top = score_once(&pattern, &lower(&pattern), &word, &word_low);
    if pattern.len() >= 3 {
        let tries = (pattern.len() - 1).min(7);
        for moving in 1..tries {
            let Some(swapped) = swap_with_next(&pattern, moving) else {
                continue;
            };
            let Some(mut candidate) = score_once(&swapped, &lower(&swapped), &word, &word_low) else {
                continue;
            };
            candidate.score -= 3;
            if top.as_ref().is_none_or(|top| candidate.score > top.score) {
                top = Some(candidate);
            }
        }
    }
    top
}

/// `pattern` with the characters at `at` and `at + 1` swapped (`nextTypoPermutation`);
/// `None` when they are the same or there is no `at + 1`.
fn swap_with_next(pattern: &[char], at: usize) -> Option<Vec<char>> {
    if at + 1 >= pattern.len() || pattern[at] == pattern[at + 1] {
        return None;
    }
    let mut swapped = pattern.to_vec();
    swapped.swap(at, at + 1);
    Some(swapped)
}

/// Each character in lower case (one that lower-cases to several stays as it is).
fn lower(text: &[char]) -> Vec<char> {
    text.iter()
        .map(|&c| {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(single), None) => single,
                _ => c,
            }
        })
        .collect()
}

fn is_upper(word: &[char], word_low: &[char], at: usize) -> bool {
    word[at] != word_low[at]
}

/// Monaco's `isSeparatorAtPos`.
fn is_separator(c: char) -> bool {
    matches!(c, '_' | '-' | '.' | ' ' | '/' | '\\' | '\'' | '"' | ':' | '$' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}')
        || is_emoji(c)
}

/// Monaco's `isEmojiImprecise`.
fn is_emoji(c: char) -> bool {
    matches!(u32::from(c),
        0x1F1E6..=0x1F1FF | 8986 | 8987 | 9200 | 9203 | 9728..=10175 | 11088 | 11093 | 127744..=128591
        | 128640..=128764 | 128992..=129008 | 129280..=129535 | 129648..=129782)
}

/// Whether the character before `at` is a separator or a space or tab.
fn follows_separator(word_low: &[char], at: usize) -> bool {
    at > 0 && (is_separator(word_low[at - 1]) || matches!(word_low[at - 1], ' ' | '\t'))
}

/// A step of the best alignment, back to where the match came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arrow {
    Diag,
    Left,
    LeftLeft,
}

/// `fuzzyScore`.
fn score_once(pattern: &[char], pattern_low: &[char], word: &[char], word_low: &[char]) -> Option<Match> {
    let pattern_len = pattern.len().min(MAX_LEN);
    let word_len = word.len().min(MAX_LEN);
    if pattern_len == 0 || word_len == 0 || pattern_len > word_len {
        return None;
    }
    // The first and the last place each typed character can match, all in order.
    let mut first = vec![0; pattern_len];
    let mut at = 0;
    for (index, &c) in word_low[..word_len].iter().enumerate() {
        if at < pattern_len && pattern_low[at] == c {
            first[at] = index;
            at += 1;
        }
    }
    if at < pattern_len {
        return None;
    }
    let mut last = vec![0; pattern_len];
    let mut left = pattern_len;
    for index in (0..word_len).rev() {
        if left > 0 && pattern_low[left - 1] == word_low[index] {
            last[left - 1] = index;
            left -= 1;
        }
    }

    // Row r is typed character r - 1, column c word character c - 1.
    let mut table = vec![vec![0i32; word_len + 1]; pattern_len + 1];
    let mut diagonal = vec![vec![0usize; word_len + 1]; pattern_len + 1];
    let mut arrows = vec![vec![Arrow::Diag; word_len + 1]; pattern_len + 1];
    let mut strong_first = false;
    for row in 1..=pattern_len {
        let pattern_pos = row - 1;
        let (min, max) = (first[pattern_pos], last[pattern_pos]);
        let next_max = if row < pattern_len { last[row] } else { word_len };
        for word_pos in min..next_max {
            let column = word_pos + 1;
            let diag_score = (word_pos <= max)
                .then(|| {
                    let new_match_start = diagonal[row - 1][column - 1] == 0;
                    let at = Position { pattern_pos, word_pos, word_len, new_match_start };
                    score_at(pattern, pattern_low, word, word_low, at, &mut strong_first)
                })
                .flatten()
                .map(|score| score + table[row - 1][column - 1]);
            let gap_start = |column: usize| if diagonal[row][column] > 0 { -5 } else { 0 };
            let can_come_left = word_pos > min;
            let left_score = if can_come_left { table[row][column - 1] + gap_start(column - 1) } else { 0 };
            let can_come_left_left = word_pos > min + 1 && diagonal[row][column - 1] > 0;
            let left_left_score =
                if can_come_left_left { table[row][column - 2] + gap_start(column - 2) } else { 0 };
            // Coming from the left means matching earlier in the word: preferred on a tie.
            if can_come_left_left
                && (!can_come_left || left_left_score >= left_score)
                && diag_score.is_none_or(|diag| left_left_score >= diag)
            {
                table[row][column] = left_left_score;
                arrows[row][column] = Arrow::LeftLeft;
                diagonal[row][column] = 0;
            } else if can_come_left && diag_score.is_none_or(|diag| left_score >= diag) {
                table[row][column] = left_score;
                arrows[row][column] = Arrow::Left;
                diagonal[row][column] = 0;
            } else {
                // A row's first column (`min`) is a match; every later one can come from the left.
                table[row][column] = diag_score.expect("a column that cannot come from the left is a match");
                arrows[row][column] = Arrow::Diag;
                diagonal[row][column] = diagonal[row - 1][column - 1] + 1;
            }
        }
    }
    if !strong_first {
        return None;
    }

    let (mut row, mut column) = (pattern_len, word_len);
    let mut score = table[row][column];
    let mut positions = Vec::with_capacity(pattern_len);
    let mut backwards_diagonal = 0;
    let mut last_match_column = 0;
    while row >= 1 {
        let mut diag_column = column;
        loop {
            match arrows[row][diag_column] {
                Arrow::LeftLeft => diag_column -= 2,
                Arrow::Left => diag_column -= 1,
                Arrow::Diag => break,
            }
            if diag_column < 1 {
                break;
            }
        }
        // Keep the contiguous match found going backwards when it is longer than the one
        // the forward pass chose.
        if backwards_diagonal > 1
            && pattern_low[row - 1] == word_low[column - 1]
            && !is_upper(word, word_low, diag_column - 1)
            && backwards_diagonal + 1 > diagonal[row][diag_column]
        {
            diag_column = column;
        }
        backwards_diagonal = if diag_column == column { backwards_diagonal + 1 } else { 1 };
        if last_match_column == 0 {
            last_match_column = diag_column;
        }
        row -= 1;
        column = diag_column - 1;
        positions.push(column);
    }
    positions.reverse();
    if word_len == pattern_len {
        // The whole word was typed.
        score += 2;
    }
    // A point off for each word character skipped.
    score -= (last_match_column - pattern_len) as i32;
    Some(Match { score, positions })
}

/// Where [`score_at`] scores.
#[derive(Debug, Clone, Copy)]
struct Position {
    pattern_pos: usize,
    word_pos: usize,
    word_len: usize,
    /// No match right before (a gap, or the start).
    new_match_start: bool,
}

/// `_doScore`: what matching the typed character at `pattern_pos` with the word character
/// at `word_pos` is worth, if they match. A strong match of the first typed character sets
/// `strong_first`.
fn score_at(
    pattern: &[char],
    pattern_low: &[char],
    word: &[char],
    word_low: &[char],
    at: Position,
    strong_first: &mut bool,
) -> Option<i32> {
    let Position { pattern_pos, word_pos, word_len, new_match_start } = at;
    if pattern_low[pattern_pos] != word_low[word_pos] {
        return None;
    }
    let same_case = pattern[pattern_pos] == word[word_pos];
    let mut score = 1;
    let mut gap_location = false;
    if word_pos == pattern_pos {
        // A common prefix.
        score = if same_case { 7 } else { 5 };
    } else if is_upper(word, word_low, word_pos) && (word_pos == 0 || !is_upper(word, word_low, word_pos - 1)) {
        // A camelCase hump.
        score = if same_case { 7 } else { 5 };
        gap_location = true;
    } else if is_separator(word_low[word_pos]) && (word_pos == 0 || !is_separator(word_low[word_pos - 1])) {
        score = 5;
    } else if follows_separator(word_low, word_pos) {
        score = 5;
        gap_location = true;
    }
    if score > 1 && pattern_pos == 0 {
        *strong_first = true;
    }
    gap_location = gap_location || is_upper(word, word_low, word_pos) || follows_separator(word_low, word_pos);
    if pattern_pos == 0 {
        if word_pos > 0 {
            score -= if gap_location { 3 } else { 5 };
        }
    } else if new_match_start {
        score += if gap_location { 2 } else { 0 };
    } else {
        score += if gap_location { 0 } else { 1 };
    }
    if word_pos + 1 == word_len {
        // As if there were a gap after the last character, not to favour matching it.
        score -= if gap_location { 3 } else { 5 };
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A score and the matched positions.
    type Scored = Option<(i32, &'static [usize])>;

    /// Monaco 0.55.1's `fuzzyScoreGracefulAggressive` on each pair: its score and matched
    /// positions.
    #[test]
    fn scores_as_monaco() {
        let cases: &[(&str, &str, Scored)] = &[
            ("bar", "fooBar", Some((-2, &[3, 4, 5]))),
            ("fb", "fooBar", Some((2, &[0, 3]))),
            ("fb", "foo_bar", Some((1, &[0, 4]))),
            ("oo", "foobar", None),
            ("fo", "foobar", Some((10, &[0, 1]))),
            ("fo", "Football", Some((8, &[0, 1]))),
            ("Fo", "food", Some((8, &[0, 1]))),
            ("food", "food", Some((28, &[0, 1, 2, 3]))),
            ("fob", "foobar", Some((5, &[0, 1, 3]))),
            ("fbo", "foobar", Some((2, &[0, 1, 3]))),
            ("cno", "console", Some((15, &[0, 1, 2]))),
            ("cnoso", "console", Some((31, &[0, 1, 2, 3, 4]))),
            ("ba", "foo_bar_baz", Some((-5, &[4, 5]))),
            ("fbb", "fooBarBaz", Some((2, &[0, 3, 6]))),
            ("abc", "ab", None),
            ("tes", "TestCase", Some((16, &[0, 1, 2]))),
            ("tc", "TestCase", Some((-1, &[0, 4]))),
            ("ss", "isSelectedSearch", Some((-10, &[2, 10]))),
            ("aaa", "aaaaa", Some((18, &[0, 1, 2]))),
            ("ab", "a-b", Some((5, &[0, 2]))),
            ("ui", "user_id", Some((0, &[0, 5]))),
            ("n", "$name", Some((-4, &[1]))),
            ("na", "$name", Some((-2, &[1, 2]))),
            ("$n", "$name", Some((9, &[0, 1]))),
            ("sel", "isSelected", Some((-1, &[2, 3, 4]))),
            ("sed", "isSelected", Some((-12, &[2, 3, 9]))),
            ("aab", "aaxaab", Some((3, &[0, 1, 5]))),
            ("abcd", "abxcdabcd", Some((7, &[0, 1, 3, 4]))),
            ("btn-p", ".btn-primary", Some((11, &[1, 2, 3, 4, 5]))),
            (".btn", ".btn-primary", Some((25, &[0, 1, 2, 3]))),
            ("lol", "LongOtherLabel", Some((3, &[0, 1, 9]))),
        ];
        for &(pattern, word, expected) in cases {
            let found = score(pattern, word).map(|found| (found.score, found.positions));
            assert_eq!(found, expected.map(|(score, positions)| (score, positions.to_vec())), "{pattern} in {word}");
        }
    }
}
