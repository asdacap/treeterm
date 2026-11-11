//! Monaco's bracket pair colorization for the code editors: `()`, `[]` and `{}` coloured by
//! nesting depth (one depth across all three kinds), skipping brackets inside strings and
//! comments; a closing bracket that closes nothing open, or the wrong kind, is unexpected.
//!
//! Monaco finds strings and comments with each language's tokenizer; this scans with the
//! comment and string syntax of the language's family, which covers the same ground for
//! the languages the viewer names. Languages Monaco has no bracket pairs for (plain text,
//! markdown, HTML) get none.

use std::ops::Range;

/// One bracket and how it pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bracket {
    /// The bracket's byte (it is one ASCII byte).
    pub range: Range<usize>,
    pub depth: Depth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// Nesting level, from 0 for the outermost pair.
    Level(usize),
    /// Closes nothing open, or closes a bracket of another kind.
    Unexpected,
}

/// How a language writes comments and strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Syntax {
    /// `//` and `/* */`; `"`, `'` and (with `backtick`) template strings with `${}`.
    CLike { line_comments: bool, backtick: bool, rust_chars: bool },
    /// `#` comments; `"`/`'` strings, tripled for Python.
    Hash,
    /// `--` comments; `"`/`'` strings (Lua, SQL).
    DoubleDash,
}

/// The scanning syntax for a Monaco language id, or `None` for one without bracket pairs.
fn syntax_of(language: &str) -> Option<Syntax> {
    let c_like = |backtick| Syntax::CLike { line_comments: true, backtick, rust_chars: false };
    Some(match language {
        "typescript" | "javascript" | "go" => c_like(true),
        "rust" => Syntax::CLike { line_comments: true, backtick: false, rust_chars: true },
        "json" | "java" | "kotlin" | "swift" | "c" | "cpp" | "csharp" | "php" | "scala" | "scss" | "less"
        | "graphql" | "dockerfile" => c_like(false),
        "css" => Syntax::CLike { line_comments: false, backtick: false, rust_chars: false },
        "python" | "shell" | "ruby" | "yaml" | "toml" | "r" => Syntax::Hash,
        "lua" | "sql" => Syntax::DoubleDash,
        _ => return None,
    })
}

/// Every bracket outside strings and comments in `text` (a file of Monaco `language`).
pub fn bracket_pairs(text: &str, language: &str) -> Vec<Bracket> {
    let Some(syntax) = syntax_of(language) else {
        return Vec::new();
    };
    Scanner { bytes: text.as_bytes(), at: 0, syntax, out: Vec::new(), open: Vec::new() }.run()
}

/// What an open `{` belongs to: code, or a template string's `${`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Open {
    Bracket(u8),
    /// `${` in a template string: its `}` resumes the string.
    Interpolation,
}

struct Scanner<'a> {
    bytes: &'a [u8],
    at: usize,
    syntax: Syntax,
    out: Vec<Bracket>,
    open: Vec<Open>,
}

impl Scanner<'_> {
    fn run(mut self) -> Vec<Bracket> {
        while self.at < self.bytes.len() {
            self.step();
        }
        self.out
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.at + offset).copied()
    }

    fn starts_with(&self, prefix: &[u8]) -> bool {
        self.bytes[self.at..].starts_with(prefix)
    }

    fn depth(&self) -> usize {
        self.open.len()
    }

    fn push(&mut self, depth: Depth) {
        self.out.push(Bracket { range: self.at..self.at + 1, depth });
    }

    fn skip_line(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at] != b'\n' {
            self.at += 1;
        }
    }

    fn skip_until(&mut self, end: &[u8]) {
        while self.at < self.bytes.len() && !self.starts_with(end) {
            self.at += 1;
        }
        self.at = (self.at + end.len()).min(self.bytes.len());
    }

    /// A quoted string from its opening quote; escapes skip a byte; ends at the quote or
    /// (unless `multiline`) at the end of the line.
    fn skip_string(&mut self, quote: u8, multiline: bool) {
        self.at += 1;
        while let Some(byte) = self.peek(0) {
            match byte {
                b'\\' => self.at += 2,
                b'\n' if !multiline => return,
                _ if byte == quote => {
                    self.at += 1;
                    return;
                }
                _ => self.at += 1,
            }
        }
    }

    /// A template string from its backtick (or from after an interpolation's `}`): stops
    /// after the closing backtick, or at `${`, which opens an interpolation.
    fn skip_template(&mut self) {
        while let Some(byte) = self.peek(0) {
            match byte {
                b'\\' => self.at += 2,
                b'`' => {
                    self.at += 1;
                    return;
                }
                b'$' if self.peek(1) == Some(b'{') => {
                    self.at += 1;
                    let depth = self.depth();
                    self.push(Depth::Level(depth));
                    self.open.push(Open::Interpolation);
                    self.at += 1;
                    return;
                }
                _ => self.at += 1,
            }
        }
    }

    fn step(&mut self) {
        let byte = self.bytes[self.at];
        match (self.syntax, byte) {
            (Syntax::CLike { line_comments: true, .. }, b'/') if self.peek(1) == Some(b'/') => return self.skip_line(),
            (Syntax::CLike { .. }, b'/') if self.peek(1) == Some(b'*') => {
                self.at += 2;
                return self.skip_until(b"*/");
            }
            (Syntax::CLike { backtick: true, .. }, b'`') => {
                self.at += 1;
                return self.skip_template();
            }
            (Syntax::CLike { rust_chars: true, .. }, b'\'') => return self.skip_rust_quote(),
            (Syntax::CLike { .. }, b'"' | b'\'') => return self.skip_string(byte, false),
            (Syntax::Hash, b'#') => return self.skip_line(),
            (Syntax::Hash, b'"' | b'\'') => {
                let triple = [byte; 3];
                if self.starts_with(&triple) {
                    self.at += 3;
                    return self.skip_until(&triple);
                }
                return self.skip_string(byte, false);
            }
            (Syntax::DoubleDash, b'-') if self.peek(1) == Some(b'-') => return self.skip_line(),
            (Syntax::DoubleDash, b'"' | b'\'') => return self.skip_string(byte, false),
            _ => {}
        }
        match byte {
            b'(' | b'[' | b'{' => {
                let depth = self.depth();
                self.push(Depth::Level(depth));
                self.open.push(Open::Bracket(byte));
            }
            b')' | b']' | b'}' => {
                let opener = match byte {
                    b')' => b'(',
                    b']' => b'[',
                    _ => b'{',
                };
                match self.open.last().copied() {
                    Some(Open::Bracket(open)) if open == opener => {
                        self.open.pop();
                        let depth = self.depth();
                        self.push(Depth::Level(depth));
                    }
                    Some(Open::Interpolation) if byte == b'}' => {
                        self.open.pop();
                        let depth = self.depth();
                        self.push(Depth::Level(depth));
                        self.at += 1;
                        return self.skip_template();
                    }
                    Some(Open::Bracket(_) | Open::Interpolation) | None => self.push(Depth::Unexpected),
                }
            }
            _ => {}
        }
        self.at += 1;
    }

    /// Rust: `'x'` / `'\n'` is a char; `'a` (a lifetime or label) is not a quote.
    fn skip_rust_quote(&mut self) {
        let char_end = match (self.peek(1), self.peek(2)) {
            (Some(b'\\'), _) => self.bytes[self.at + 2..].iter().take(10).position(|&b| b == b'\'').map(|end| end + 3),
            (Some(_), _) => {
                // One (possibly multi-byte) character, then the closing quote.
                let rest = &self.bytes[self.at + 1..];
                let len = std::str::from_utf8(&rest[..rest.len().min(4)])
                    .or_else(|error| std::str::from_utf8(&rest[..error.valid_up_to()]))
                    .ok()
                    .and_then(|s| s.chars().next())
                    .map_or(1, char::len_utf8);
                (rest.get(len) == Some(&b'\'')).then_some(len + 2)
            }
            (None, _) => None,
        };
        self.at += char_end.unwrap_or(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depths(text: &str, language: &str) -> Vec<(char, Option<usize>)> {
        bracket_pairs(text, language)
            .into_iter()
            .map(|b| {
                let depth = match b.depth {
                    Depth::Level(level) => Some(level),
                    Depth::Unexpected => None,
                };
                (text.as_bytes()[b.range.start] as char, depth)
            })
            .collect()
    }

    #[test]
    fn brackets_nest_with_one_depth_for_every_kind() {
        assert_eq!(
            depths("f(a[1], { b: (2) })", "typescript"),
            vec![('(', Some(0)), ('[', Some(1)), (']', Some(1)), ('{', Some(1)), ('(', Some(2)), (')', Some(2)), ('}', Some(1)), (')', Some(0))]
        );
    }

    #[test]
    fn strings_and_comments_hide_their_brackets() {
        assert_eq!(depths("a(\"(\", '[') // )\n/* { */ }", "javascript"), vec![('(', Some(0)), (')', Some(0)), ('}', None)]);
        assert_eq!(depths("x = '(' # (\ny = \"\"\"\n[\n\"\"\" + [1]", "python"), vec![('[', Some(0)), (']', Some(0))]);
        assert_eq!(depths("select ('(') -- )", "sql"), vec![('(', Some(0)), (')', Some(0))]);
    }

    #[test]
    fn template_interpolations_are_code() {
        assert_eq!(
            depths("f(`a ${g(1)} (b) ${x}`)", "typescript"),
            vec![('(', Some(0)), ('{', Some(1)), ('(', Some(2)), (')', Some(2)), ('}', Some(1)), ('{', Some(1)), ('}', Some(1)), (')', Some(0))]
        );
    }

    #[test]
    fn rust_lifetimes_are_not_quotes() {
        assert_eq!(
            depths("fn f<'a>(x: &'a str) -> char { '(' }", "rust"),
            vec![('(', Some(0)), (')', Some(0)), ('{', Some(0)), ('}', Some(0))]
        );
        assert_eq!(depths("['\\'', 'é']", "rust"), vec![('[', Some(0)), (']', Some(0))]);
    }

    #[test]
    fn mismatched_closers_are_unexpected_and_plain_text_has_none() {
        assert_eq!(depths("(]", "c"), vec![('(', Some(0)), (']', None)]);
        assert!(bracket_pairs("(a)", "plaintext").is_empty());
        assert!(bracket_pairs("[a](b)", "markdown").is_empty());
    }
}
