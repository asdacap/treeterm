//! What Monaco's CSS worker (vscode-css-languageservice) adds to CSS, SCSS and Less files,
//! without its parser: the colours it finds in declaration values (`findDocumentColors`),
//! property names to complete inside a rule, and the warnings of its lint that read rules
//! and declarations (CSS files only, with Monaco's default lint settings).
//!
//! The file is read as a tree of rules (`selector { … }`), at-rules and declarations
//! (`property: value`), split at `{`, `}` and `;` outside strings, comments and brackets.
//! The worker's parse errors, and the lint rules that need its parse of values and
//! selectors, are not reported.

use std::ops::Range;

use super::web_data::{CSS_COLORS, CSS_PROPERTIES};
use super::{Completion, Completions, Problem, Severity};
use crate::words::{self, Trigger};

/// The style sheet languages the CSS worker reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Css,
    Scss,
    Less,
}

impl Dialect {
    /// The dialect of Monaco's `language`, if the CSS worker reads it.
    pub fn of(language: &str) -> Option<Self> {
        match language {
            "css" => Some(Self::Css),
            "scss" => Some(Self::Scss),
            "less" => Some(Self::Less),
            _ => None,
        }
    }

    /// SCSS and Less have `//` line comments.
    fn has_line_comments(self) -> bool {
        match self {
            Self::Css => false,
            Self::Scss | Self::Less => true,
        }
    }
}

/// What ends a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    Semicolon,
    /// The `}` of its block, or the end of the text.
    Block,
}

/// `property: value`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Declaration {
    property: Range<usize>,
    /// Trimmed; empty when there is no value.
    value: Range<usize>,
    end: End,
}

impl Declaration {
    /// From the property to the end of the value.
    fn range(&self) -> Range<usize> {
        self.property.start..self.value.end.max(self.property.end)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Declaration(Declaration),
    /// `selectors { … }`.
    Rule { selectors: Range<usize>, body: Vec<Item> },
    /// `@name prelude;` or `@name prelude { … }`, from `@` to the end of the statement.
    AtRule { name: Range<usize>, range: Range<usize>, body: Option<Vec<Item>> },
}

/// Reads the rules, at-rules and declarations of a style sheet.
struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    dialect: Dialect,
}

impl Reader<'_> {
    /// Past a comment starting at `pos`, if one does.
    fn skip_comment(&self, pos: usize) -> Option<usize> {
        let rest = &self.text[pos..];
        if let Some(comment) = rest.strip_prefix("/*") {
            return Some(comment.find("*/").map_or(self.text.len(), |end| pos + 2 + end + 2));
        }
        if self.dialect.has_line_comments() && rest.starts_with("//") {
            return Some(rest.find('\n').map_or(self.text.len(), |end| pos + end));
        }
        None
    }

    /// Past the whitespace and comments at `pos`.
    fn skip_trivia(&self, mut pos: usize) -> usize {
        loop {
            while pos < self.bytes.len() && self.bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            match self.skip_comment(pos) {
                Some(after) => pos = after,
                None => return pos,
            }
        }
    }

    /// Past the string quoted by the character at `pos`.
    fn skip_string(&self, pos: usize) -> usize {
        let quote = self.bytes[pos];
        let mut at = pos + 1;
        while at < self.bytes.len() {
            match self.bytes[at] {
                b'\\' => at += 2,
                b'\n' => return at,
                byte if byte == quote => return at + 1,
                _ => at += 1,
            }
        }
        self.bytes.len()
    }

    /// Where the statement at `pos` ends: the first `;`, `{` or `}` outside strings,
    /// comments and brackets (or the end of the text).
    fn statement_end(&self, mut pos: usize) -> usize {
        let mut depth = 0usize;
        while pos < self.bytes.len() {
            if depth == 0
                && let Some(after) = self.skip_comment(pos)
            {
                pos = after;
                continue;
            }
            match self.bytes[pos] {
                b'"' | b'\'' => {
                    pos = self.skip_string(pos);
                    continue;
                }
                b'\\' => pos += 1,
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth = depth.saturating_sub(1),
                b';' | b'{' | b'}' if depth == 0 => return pos,
                _ => {}
            }
            pos += 1;
        }
        self.bytes.len()
    }

    /// Where a custom property's value from `pos` ends: it may hold blocks (`--x: { a: b }`).
    fn custom_value_end(&self, mut pos: usize) -> usize {
        let mut depth = 0usize;
        while pos < self.bytes.len() {
            let end = self.statement_end(pos);
            match self.bytes.get(end) {
                Some(b'{') => depth += 1,
                Some(b'}') if depth > 0 => depth -= 1,
                Some(_) | None => return end,
            }
            pos = end + 1;
        }
        self.bytes.len()
    }

    /// `range` without the whitespace and comments around it.
    fn trim(&self, range: Range<usize>) -> Range<usize> {
        let text = &self.text[range.clone()];
        let start = range.start + (text.len() - text.trim_start().len());
        let end = range.start + text.trim_end().len();
        start..end.max(start)
    }

    /// The first `:` of a statement outside strings and brackets.
    fn colon(&self, range: Range<usize>) -> Option<usize> {
        let mut depth = 0usize;
        let mut pos = range.start;
        while pos < range.end {
            match self.bytes[pos] {
                b'"' | b'\'' => {
                    pos = self.skip_string(pos);
                    continue;
                }
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth = depth.saturating_sub(1),
                b':' if depth == 0 => return Some(pos),
                _ => {}
            }
            pos += 1;
        }
        None
    }

    /// The items from `pos` up to the `}` that closes them (or the end of the text), and
    /// where that is.
    fn items(&self, mut pos: usize, nested: bool) -> (Vec<Item>, usize) {
        let mut items = Vec::new();
        loop {
            pos = self.skip_trivia(pos);
            match self.bytes.get(pos) {
                None => return (items, pos),
                Some(b'}') if nested => return (items, pos),
                // A stray `}` at the top: skipped.
                Some(b'}' | b';') => {
                    pos += 1;
                    continue;
                }
                Some(_) => {}
            }
            let start = pos;
            let mut end = self.statement_end(pos);
            if self.bytes.get(end) == Some(&b'{')
                && self.text[start..end].starts_with("--")
                && let Some(colon) = self.colon(start..end)
            {
                end = self.custom_value_end(colon + 1);
            }
            let statement = self.trim(start..end);
            let name = || {
                let length = self.text[start + 1..statement.end]
                    .find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
                    .unwrap_or(statement.end - start - 1);
                start..start + 1 + length
            };
            // A Less variable (`@name: value`) is a declaration.
            let at_rule = self.bytes[start] == b'@'
                && !(self.dialect == Dialect::Less && self.text[name().end..].starts_with(':'));
            if self.bytes.get(end) == Some(&b'{') {
                let (body, close) = self.items(end + 1, true);
                let after = (close + 1).min(self.text.len());
                items.push(match at_rule {
                    true => Item::AtRule { name: name(), range: start..after, body: Some(body) },
                    false => Item::Rule { selectors: statement, body },
                });
                pos = after;
                continue;
            }
            if at_rule {
                items.push(Item::AtRule { name: name(), range: statement, body: None });
            } else if let Some(colon) = self.colon(statement.clone()) {
                let property = self.trim(statement.start..colon);
                let value = self.trim(colon + 1..statement.end);
                let end = if self.bytes.get(end) == Some(&b';') { End::Semicolon } else { End::Block };
                items.push(Item::Declaration(Declaration { property, value, end }));
            }
            pos = if self.bytes.get(end) == Some(&b';') { end + 1 } else { end };
        }
    }
}

fn read(text: &str, dialect: Dialect) -> Vec<Item> {
    Reader { text, bytes: text.as_bytes(), dialect }.items(0, false).0
}

/// Every declaration of `items`, nested ones included, in order.
fn declarations(items: &[Item], found: &mut Vec<Declaration>) {
    for item in items {
        match item {
            Item::Declaration(declaration) => found.push(declaration.clone()),
            Item::Rule { body, .. } | Item::AtRule { body: Some(body), .. } => declarations(body, found),
            Item::AtRule { body: None, .. } => {}
        }
    }
}

/// A colour, each channel from 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

fn hex_digit(byte: u8) -> f32 {
    f32::from((byte as char).to_digit(16).map_or(0, |digit| digit as u8))
}

/// `colorFromHex`: `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
fn color_from_hex(text: &str) -> Option<Rgba> {
    let digits = text.strip_prefix('#')?.as_bytes();
    let short = |index: usize| hex_digit(digits[index]) * 17. / 255.;
    let long = |index: usize| (hex_digit(digits[index]) * 16. + hex_digit(digits[index + 1])) / 255.;
    match digits.len() {
        3 => Some(Rgba { red: short(0), green: short(1), blue: short(2), alpha: 1. }),
        4 => Some(Rgba { red: short(0), green: short(1), blue: short(2), alpha: short(3) }),
        6 => Some(Rgba { red: long(0), green: long(2), blue: long(4), alpha: 1. }),
        8 => Some(Rgba { red: long(0), green: long(2), blue: long(4), alpha: long(6) }),
        _ => None,
    }
}

/// The number `^([-+]?[0-9]*\.?[0-9]+)` starts `text` with, and the rest.
fn leading_number(text: &str) -> Option<(f64, &str)> {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let digits = unsigned.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(unsigned.len());
    let number = &unsigned[..digits];
    // At most one point, and a digit after it.
    let valid = match number.split_once('.') {
        Some((_, fraction)) => !fraction.is_empty() && !fraction.contains('.'),
        None => !number.is_empty(),
    };
    if !valid {
        return None;
    }
    let end = text.len() - unsigned.len() + digits;
    Some((text[..end].parse().ok()?, &text[end..]))
}

/// `getNumericValue`: a number or percentage, as a fraction of `factor`, from 0 to 1.
fn numeric_value(text: &str, factor: f64) -> Option<f64> {
    let (number, rest) = leading_number(text)?;
    let factor = match rest {
        "%" => 100.,
        "" => factor,
        _ => return None,
    };
    let value = number / factor;
    (0. ..=1.).contains(&value).then_some(value)
}

/// `getAngle`: degrees.
fn angle(text: &str) -> Option<f64> {
    let (number, unit) = leading_number(text)?;
    let degrees = match unit {
        "" | "deg" => number,
        "rad" => number * 180. / std::f64::consts::PI,
        "grad" => number * 0.9,
        "turn" => number * 360.,
        _ => return None,
    };
    Some(degrees % 360.)
}

/// `colorFromHSL`, each argument from 0 to 1 but the hue, in degrees.
fn color_from_hsl(hue: f64, saturation: f64, light: f64, alpha: f64) -> [f64; 4] {
    let hue = hue / 60.;
    if saturation == 0. {
        return [light, light, light, alpha];
    }
    let hue_to_rgb = |t1: f64, t2: f64, mut hue: f64| {
        while hue < 0. {
            hue += 6.;
        }
        while hue >= 6. {
            hue -= 6.;
        }
        if hue < 1. {
            (t2 - t1) * hue + t1
        } else if hue < 3. {
            t2
        } else if hue < 4. {
            (t2 - t1) * (4. - hue) + t1
        } else {
            t1
        }
    };
    let t2 = if light <= 0.5 { light * (saturation + 1.) } else { light + saturation - light * saturation };
    let t1 = light * 2. - t2;
    [hue_to_rgb(t1, t2, hue + 2.), hue_to_rgb(t1, t2, hue), hue_to_rgb(t1, t2, hue - 2.), alpha]
}

/// `colorFromHWB`.
fn color_from_hwb(hue: f64, white: f64, black: f64, alpha: f64) -> [f64; 4] {
    if white + black >= 1. {
        let gray = white / (white + black);
        return [gray, gray, gray, alpha];
    }
    let [red, green, blue, _] = color_from_hsl(hue, 1., 0.5, alpha);
    let mix = |channel: f64| channel * (1. - white - black) + white;
    [mix(red), mix(green), mix(blue), alpha]
}

/// The arguments of a colour function as the worker reads them: comma-separated, or (one
/// argument) space-separated terms, `a / b` being one; when that is the third, its two
/// sides are the third value and the alpha.
fn color_arguments(arguments: &str) -> Vec<String> {
    let split: Vec<&str> = arguments.split(',').collect();
    if split.len() != 1 {
        return split.into_iter().map(|argument| argument.trim().to_owned()).collect();
    }
    let spaced = arguments.replace('/', " / ");
    let mut terms: Vec<String> = Vec::new();
    let mut parts = spaced.split_whitespace();
    while let Some(part) = parts.next() {
        if part == "/"
            && let Some(last) = terms.last_mut()
            && let Some(next) = parts.next()
        {
            last.push('/');
            last.push_str(next);
            continue;
        }
        terms.push(part.to_owned());
    }
    if let [first, second, third] = &terms[..]
        && let Some((left, right)) = third.split_once('/')
    {
        return vec![first.clone(), second.clone(), left.to_owned(), right.to_owned()];
    }
    terms
}

/// `getColorValue` for a function call: `rgb()`, `rgba()`, `hsl()`, `hsla()` or `hwb()`.
fn color_from_function(name: &str, arguments: &str) -> Option<Rgba> {
    let values = color_arguments(arguments);
    let values: Vec<&str> = values.iter().map(String::as_str).collect();
    if !(3..=4).contains(&values.len()) {
        return None;
    }
    let alpha = match values.get(3) {
        Some(alpha) => numeric_value(alpha, 1.)?,
        None => 1.,
    };
    let [red, green, blue, alpha] = match name {
        "rgb" | "rgba" => {
            [numeric_value(values[0], 255.)?, numeric_value(values[1], 255.)?, numeric_value(values[2], 255.)?, alpha]
        }
        "hsl" | "hsla" => {
            color_from_hsl(angle(values[0])?, numeric_value(values[1], 100.)?, numeric_value(values[2], 100.)?, alpha)
        }
        "hwb" => color_from_hwb(angle(values[0])?, numeric_value(values[1], 100.)?, numeric_value(values[2], 100.)?, alpha),
        _ => return None,
    };
    Some(Rgba { red: red as f32, green: green as f32, blue: blue as f32, alpha: alpha as f32 })
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte >= 0x80
}

/// The colours in a declaration's `value` (bytes `range` of `text`).
fn value_colors(text: &str, range: Range<usize>, found: &mut Vec<(Range<usize>, Rgba)>) {
    let bytes = text.as_bytes();
    let mut pos = range.start;
    let name_end = |from: usize| (from..range.end).find(|&at| !is_name_byte(bytes[at])).unwrap_or(range.end);
    while pos < range.end {
        let byte = bytes[pos];
        match byte {
            b'"' | b'\'' => {
                let mut at = pos + 1;
                while at < range.end && bytes[at] != byte {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
                pos = (at + 1).min(range.end);
            }
            b'/' if text[pos..range.end].starts_with("/*") => {
                pos = text[pos + 2..range.end].find("*/").map_or(range.end, |end| pos + 2 + end + 2);
            }
            b'#' => {
                let end = name_end(pos + 1);
                let hex = &text[pos..end];
                if matches!(hex.len(), 4 | 5 | 7 | 9) && hex[1..].bytes().all(|b| b.is_ascii_hexdigit())
                    && let Some(color) = color_from_hex(hex)
                {
                    found.push((pos..end, color));
                }
                pos = end.max(pos + 1);
            }
            // A number and its unit.
            b'0'..=b'9' | b'.' => pos = name_end(pos + 1),
            // A variable (`$name`, `@name`): never a colour.
            b'$' | b'@' => pos = name_end(pos + 1),
            byte if is_name_byte(byte) => {
                let end = name_end(pos);
                let name = &text[pos..end];
                if bytes.get(end) == Some(&b'(') {
                    // The call, to its closing parenthesis.
                    let mut depth = 0;
                    let mut close = range.end;
                    for (at, &b) in bytes.iter().enumerate().take(range.end).skip(end) {
                        match b {
                            b'(' => depth += 1,
                            b')' => {
                                depth -= 1;
                                if depth == 0 {
                                    close = at;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    if name.eq_ignore_ascii_case("url") {
                        pos = (close + 1).min(range.end);
                        continue;
                    }
                    if close < range.end
                        && let Some(color) = color_from_function(name, &text[end + 1..close])
                    {
                        found.push((pos..close + 1, color));
                    }
                    // Colours inside the arguments too (`linear-gradient(red, blue)`).
                    pos = end + 1;
                    continue;
                }
                let named = CSS_COLORS.iter().find(|(color, _)| color.eq_ignore_ascii_case(name));
                if let Some(color) = named.and_then(|(_, hex)| color_from_hex(hex)) {
                    found.push((pos..end, color));
                }
                pos = end;
            }
            _ => pos += 1,
        }
    }
}

/// The colours Monaco decorates in `text`, a style sheet of Monaco's `language`, by start.
pub fn colors(language: &str, text: &str) -> Vec<(Range<usize>, Rgba)> {
    let Some(dialect) = Dialect::of(language) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut all = Vec::new();
    declarations(&read(text, dialect), &mut all);
    for declaration in all {
        // The worker reads no colours in a custom property its block ends.
        if text[declaration.property.clone()].starts_with("--") && declaration.end == End::Block {
            continue;
        }
        value_colors(text, declaration.value, &mut found);
    }
    found
}

/// The at-rules the worker knows: those its parser reads, and its data's at-directives.
const KNOWN_AT_RULES: &[&str] = &[
    "@import",
    "@media",
    "@page",
    "@font-face",
    "@supports",
    "@viewport",
    "@-ms-viewport",
    "@-o-viewport",
    "@namespace",
    "@-moz-document",
    "@layer",
    "@container",
    "@property",
    "@charset",
    "@counter-style",
    "@font-feature-values",
];

/// `@keyframes`, with or without a vendor prefix (`keyframeRegex`).
fn is_keyframes(name: &str) -> bool {
    ["@keyframes", "@-webkit-keyframes", "@-ms-keyframes", "@-moz-keyframes", "@-o-keyframes"]
        .iter()
        .any(|keyframes| keyframes.eq_ignore_ascii_case(name))
}

fn is_known_property(name: &str) -> bool {
    CSS_PROPERTIES.iter().any(|(property, _)| property.eq_ignore_ascii_case(name))
}

/// The lint warnings of `items`, nested ones included.
fn lint(text: &str, items: &[Item], problems: &mut Vec<Problem>) {
    let warn = |problems: &mut Vec<Problem>, range: Range<usize>, message: String| {
        problems.push(Problem { range, severity: Severity::Warning, message });
    };
    for item in items {
        match item {
            Item::Declaration(_) => {}
            Item::Rule { selectors, body } => {
                if body.is_empty() {
                    warn(problems, selectors.clone(), "Do not use empty rulesets".to_owned());
                }
                let own: Vec<&Declaration> = body
                    .iter()
                    .filter_map(|item| match item {
                        Item::Declaration(declaration) => Some(declaration),
                        Item::Rule { .. } | Item::AtRule { .. } => None,
                    })
                    .collect();
                lint_declarations(text, &own, problems);
                // `:export` blocks (CSS modules) name values, not properties.
                if &text[selectors.clone()] != ":export" {
                    for declaration in &own {
                        let name = &text[declaration.property.clone()];
                        // Vendor-specific and custom properties are not checked; `_name` is the
                        // IE hack for `name`.
                        if declaration.value.is_empty() || name.starts_with(['-', '*']) {
                            continue;
                        }
                        let unhacked = name.strip_prefix('_').unwrap_or(name);
                        if !is_known_property(name) && !is_known_property(unhacked) {
                            warn(problems, declaration.property.clone(), format!("Unknown property: '{name}'"));
                        }
                    }
                }
                lint(text, body, problems);
            }
            Item::AtRule { name, range, body } => {
                let at_rule = &text[name.clone()];
                if at_rule.eq_ignore_ascii_case("@font-face")
                    && let Some(body) = body
                {
                    let defines = |property: &str| {
                        body.iter().any(|item| match item {
                            Item::Declaration(declaration) => {
                                !declaration.value.is_empty() && text[declaration.property.clone()].eq_ignore_ascii_case(property)
                            }
                            Item::Rule { .. } | Item::AtRule { .. } => false,
                        })
                    };
                    if !defines("src") || !defines("font-family") {
                        warn(problems, range.clone(), "@font-face rule must define 'src' and 'font-family' properties".to_owned());
                    }
                }
                let known = is_keyframes(at_rule) || KNOWN_AT_RULES.iter().any(|known| known.eq_ignore_ascii_case(at_rule));
                if !known {
                    warn(problems, name.clone(), format!("Unknown at rule {at_rule}"));
                }
                // A keyframe's blocks (`from { … }`) are not rules.
                if let Some(body) = body
                    && !is_keyframes(at_rule)
                {
                    lint(text, body, problems);
                }
            }
        }
    }
}

/// "Do not use duplicate style definitions", on each declaration of a property given twice
/// in a rule (but `background`, and values starting with `-`, the vendor fallbacks).
fn lint_declarations(text: &str, declarations: &[&Declaration], problems: &mut Vec<Problem>) {
    let name = |declaration: &Declaration| text[declaration.property.clone()].to_lowercase();
    let counts = |declaration: &&&Declaration| !declaration.value.is_empty() && !text[declaration.value.clone()].starts_with('-');
    for declaration in declarations.iter().filter(counts) {
        let property = name(declaration);
        if property == "background" {
            continue;
        }
        let others = declarations.iter().filter(counts).filter(|other| !std::ptr::eq(**other, *declaration) && name(other) == property);
        if others.count() > 0 {
            problems.push(Problem {
                range: declaration.range(),
                severity: Severity::Warning,
                message: "Do not use duplicate style definitions".to_owned(),
            });
        }
    }
}

/// The warnings Monaco's CSS lint gives `text`, a `css` file, by start.
pub fn problems(text: &str) -> Vec<Problem> {
    let mut found = Vec::new();
    lint(text, &read(text, Dialect::Css), &mut found);
    found.sort_by_key(|problem| problem.range.start);
    found
}

/// What a block holds, for completing in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    /// A rule, or an at-rule of declarations (`@font-face`, `@page`).
    Declarations,
    /// Another at-rule's: rules.
    Rules,
}

/// Where the statement the cursor (byte `offset`) is in starts, and the block it is in;
/// `None` at the top of the style sheet.
fn statement_at(reader: &Reader, offset: usize) -> Option<(usize, Block)> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut pos = 0;
    loop {
        pos = reader.skip_trivia(pos);
        let end = reader.statement_end(pos);
        if pos >= offset || end >= offset || end >= reader.text.len() {
            return blocks.last().map(|block| (pos.min(offset), *block));
        }
        match reader.bytes[end] {
            b'{' => {
                let name = reader.text[pos..end].split_whitespace().next().unwrap_or("");
                let declarations = !name.starts_with('@')
                    || name.eq_ignore_ascii_case("@font-face")
                    || name.eq_ignore_ascii_case("@page");
                blocks.push(if declarations { Block::Declarations } else { Block::Rules });
            }
            b'}' => {
                blocks.pop();
            }
            _ => {}
        }
        pos = end + 1;
    }
}

/// The property names the CSS worker completes at byte `offset` of `text`, a style sheet of
/// Monaco's `language`: inside a rule, where a property name is being typed (or asked for
/// with nothing typed, but not right after a `;`). `None` elsewhere, where Monaco's word
/// suggestions take over.
pub fn property_completions(language: &str, text: &str, offset: usize, trigger: Trigger) -> Option<Completions> {
    let dialect = Dialect::of(language)?;
    let reader = Reader { text, bytes: text.as_bytes(), dialect };
    let (start, block) = statement_at(&reader, offset)?;
    if block != Block::Declarations || !text[start..offset].bytes().all(is_name_byte) {
        return None;
    }
    let end = (offset..text.len()).find(|&at| !is_name_byte(reader.bytes[at])).unwrap_or(text.len());
    let typed = &text[start..offset];
    if typed.is_empty() && (trigger == Trigger::Typing || text[..offset].ends_with(';')) {
        return None;
    }
    let colon_follows = text[end..].trim_start_matches([' ', '\t']).starts_with(':');
    let suffix = if colon_follows { "" } else { ": " };
    let mut ranked: Vec<(i32, String, &str)> = CSS_PROPERTIES
        .iter()
        .filter_map(|(name, relevance)| {
            let score = if typed.is_empty() { 0 } else { words::fuzzy::score(typed, name)?.score };
            // The worker's `sortText`: vendor-prefixed last, then by relevance.
            let group = if name.starts_with('-') { 'x' } else { 'd' };
            Some((score, format!("{group}_{:x}", 255 - u32::from(*relevance)), *name))
        })
        .collect();
    ranked.sort_by(|(score_a, sort_a, a), (score_b, sort_b, b)| {
        score_b.cmp(score_a).then_with(|| sort_a.cmp(sort_b)).then_with(|| a.cmp(b))
    });
    let items = ranked
        .into_iter()
        .map(|(_, _, name)| Completion { label: name.to_owned(), insert: format!("{name}{suffix}") })
        .collect();
    Some(Completions { replace: start..end, items })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The colours as `(text, [red, green, blue, alpha] in 1/1000)`, by start.
    fn found(language: &str, text: &str) -> Vec<(String, [i32; 4])> {
        let thousandths = |channel: f32| (channel * 1000.).round() as i32;
        colors(language, text)
            .into_iter()
            .map(|(range, c)| {
                (text[range].to_owned(), [thousandths(c.red), thousandths(c.green), thousandths(c.blue), thousandths(c.alpha)])
            })
            .collect()
    }

    fn color(text: &str, rgba: [i32; 4]) -> (String, [i32; 4]) {
        (text.to_owned(), rgba)
    }

    /// Colours as Monaco's CSS worker finds them (checked against vscode-css-languageservice).
    #[test]
    fn colours_are_found_in_every_syntax_the_worker_reads() {
        let text = "a { color: RED; b: #abc #abcd #aabbcc #aabbcc80; \
                    c: rgb(255, 0, 0) rgba(0, 0, 0, 50%) rgb(0 0 0 / 50%) rgb(10%, 20%, 30%); \
                    d: hsla(0.5turn, 100%, 50%, .5) hwb(0 0% 0%); \
                    e: linear-gradient(red, blue); }";
        assert_eq!(found("css", text), [
            color("RED", [1000, 0, 0, 1000]),
            color("#abc", [667, 733, 800, 1000]),
            color("#abcd", [667, 733, 800, 867]),
            color("#aabbcc", [667, 733, 800, 1000]),
            color("#aabbcc80", [667, 733, 800, 502]),
            color("rgb(255, 0, 0)", [1000, 0, 0, 1000]),
            color("rgba(0, 0, 0, 50%)", [0, 0, 0, 500]),
            color("rgb(0 0 0 / 50%)", [0, 0, 0, 500]),
            color("rgb(10%, 20%, 30%)", [100, 200, 300, 1000]),
            color("hsla(0.5turn, 100%, 50%, .5)", [0, 1000, 1000, 500]),
            color("hwb(0 0% 0%)", [1000, 0, 0, 1000]),
            color("red", [1000, 0, 0, 1000]),
            color("blue", [0, 0, 1000, 1000]),
        ]);
    }

    #[test]
    fn what_the_worker_does_not_read_as_a_colour_is_not_one() {
        // Out of range, a function name in capitals, an invalid hex length, words holding a
        // colour's name, URLs, strings and comments, selectors and at-rule preludes.
        let text = "@media (color: red) { .red { a: rgb(300, 0, 0) Rgb(1, 2, 3) #ab red-ish \
                    url(#fff) 'red' /* red */; } }";
        assert_eq!(found("css", text), []);
        // A custom property its block ends (no `;`) holds none; one with a `;` does.
        assert_eq!(found("css", ":root { --a: red; --b: blue }"), [color("red", [1000, 0, 0, 1000])]);
        assert_eq!(found("plaintext", "a { color: red; }"), []);
    }

    #[test]
    fn scss_and_less_variables_and_comments_are_read_as_such() {
        let text = "$base: #fff;\n// red\n.a { color: $base; b: darken($red, 10%) blue; }";
        assert_eq!(found("scss", text), [color("#fff", [1000, 1000, 1000, 1000]), color("blue", [0, 0, 1000, 1000])]);
        assert_eq!(found("less", "@c: red;\n.a { color: @c; }"), [color("red", [1000, 0, 0, 1000])]);
    }

    /// The warnings as `(text marked, message)`, by start.
    fn warned(text: &str) -> Vec<(String, String)> {
        problems(text).into_iter().map(|problem| (text[problem.range].to_owned(), problem.message)).collect()
    }

    fn warning(at: &str, message: &str) -> (String, String) {
        (at.to_owned(), message.to_owned())
    }

    /// Monaco's lint warnings for each of these (checked against vscode-css-languageservice
    /// with Monaco's default lint settings).
    #[test]
    fn the_lint_warns_as_monaco_does() {
        assert_eq!(warned("a {} /* x */ b { /* c */ }\n@media screen { .c { } }"), [
            warning("a", "Do not use empty rulesets"),
            warning("b", "Do not use empty rulesets"),
            warning(".c", "Do not use empty rulesets"),
        ]);
        assert_eq!(warned("a { COLOR: red; Colr: x; _height: 1px; -webkit-foo: 1; --x: 1; }"), [warning(
            "Colr",
            "Unknown property: 'Colr'"
        )]);
        assert_eq!(warned("a { color: red !important; COLOR: blue; background: x; background: y; width: -1px; width: 2px }"), [
            warning("color: red !important", "Do not use duplicate style definitions"),
            warning("COLOR: blue", "Do not use duplicate style definitions"),
        ]);
        assert_eq!(warned("a { b { } c: d; }"), [
            warning("b", "Do not use empty rulesets"),
            warning("c", "Unknown property: 'c'"),
        ]);
        assert_eq!(warned("@font-face { font-family: X; }"), [warning(
            "@font-face { font-family: X; }",
            "@font-face rule must define 'src' and 'font-family' properties"
        )]);
        assert_eq!(warned("@tailwind base;\n@media x { @foo { } }"), [
            warning("@tailwind", "Unknown at rule @tailwind"),
            warning("@foo", "Unknown at rule @foo"),
        ]);
    }

    #[test]
    fn keyframes_exports_and_known_at_rules_are_not_warned_about() {
        assert_eq!(warned("@keyframes k { from { } to { a: b } }"), []);
        assert_eq!(warned(":export { foo: bar }"), []);
        assert_eq!(warned("@charset \"utf-8\";\n@import url(x.css);\n@-webkit-keyframes k { }\n@font-face { src: url(x); font-family: X }"), []);
        // A custom property's value may hold a block.
        assert_eq!(warned(":root { --x: { a: b }; --y: 1px; }"), []);
    }

    /// The labels offered at `|` in `text`, and the text a pick replaces.
    fn offered(language: &str, text: &str, trigger: Trigger) -> Option<(String, Vec<String>, Vec<String>)> {
        let offset = text.find('|').expect("a cursor");
        let text = text.replace('|', "");
        let completions = property_completions(language, &text, offset, trigger)?;
        let (labels, inserts) = completions.items.into_iter().take(3).map(|item| (item.label, item.insert)).unzip();
        Some((text[completions.replace].to_owned(), labels, inserts))
    }

    #[test]
    fn property_names_are_completed_where_a_property_is_typed() {
        let (replace, labels, inserts) = offered("css", "a { colo|r }", Trigger::Typing).expect("properties");
        assert_eq!(replace, "color");
        assert_eq!(labels[0], "color");
        assert_eq!(inserts[0], "color: ");
        // A colon already there is kept.
        let (_, labels, inserts) = offered("scss", ".a { .b { backgr|: red; } }", Trigger::Typing).expect("properties");
        assert_eq!(inserts[0], labels[0]);
        // Asked for with nothing typed: all of them, the most relevant first, vendor-prefixed last.
        let (replace, labels, _) = offered("less", "a { color: red; | }", Trigger::Invoked).expect("properties");
        assert_eq!((replace.as_str(), &labels[..]), ("", &["display", "height", "width"].map(str::to_owned)[..]));
        let all = property_completions("css", "a {  }", 4, Trigger::Invoked).expect("properties").items;
        assert_eq!(all.len(), CSS_PROPERTIES.len());
        assert!(all.last().is_some_and(|item| item.label.starts_with('-')));
    }

    #[test]
    fn property_names_are_not_completed_elsewhere() {
        // In a value, a selector at the top or in `@media`, right after a `;`, and with
        // nothing typed while typing.
        assert_eq!(offered("css", "a { color: r| }", Trigger::Typing), None);
        assert_eq!(offered("css", "di|", Trigger::Typing), None);
        assert_eq!(offered("css", "@media x { di| }", Trigger::Typing), None);
        assert_eq!(offered("css", "a { color: red;| }", Trigger::Invoked), None);
        assert_eq!(offered("css", "a { | }", Trigger::Typing), None);
        assert_eq!(offered("html", "a { co| }", Trigger::Typing), None);
    }
}
