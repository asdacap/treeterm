//! The language each part of a line is in, as Monaco's tokenizers read it, for the word under
//! the cursor ([`super::word_at`]). Monaco reads that word with the word pattern of the
//! language of the token under the cursor (`getWordAtPosition`), and its HTML, PHP and
//! Markdown tokenizers hand parts of a file to other languages: a `<script>` block to
//! JavaScript (or the language its `type` names), a `<style>` block to CSS (or its `type`'s),
//! the code between PHP's `<?php` and `?>` back to PHP, and a Markdown code fence to the
//! language it names.
//!
//! These are Monaco 0.55.1's Monarch tokenizers (`basic-languages/{html,php,markdown}`) without
//! their token types, run as its Monarch lexer runs them (`monarchLexer.js`): a state's rules
//! tried in order where the line has got to (a character no rule matches is skipped), state
//! names that carry arguments after dots (`$S2`), the match's groups substituted (`$1`),
//! `@rematch`, an embedded language that runs up to the first match, anywhere on the line, of
//! a rule leaving it, and a line the tokenizer fails on read as the file's language
//! (`safeTokenize`). JavaScript's `\w`, `\d` and `\b` are ASCII, so they are spelled out; the
//! two Markdown rules with a lookahead are written without it, for the same text.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use regex::{Captures, Regex};

/// Monaco does not tokenize a line at least this long (UTF-16 code units): it is all in the
/// tokenizer's own language (`editor.maxTokenizationLineLength`).
const MAX_TOKENIZATION_LINE_LENGTH: usize = 20_000;

/// How deep languages may nest (a Markdown code fence of Markdown, in one of Markdown, ...)
/// before the line counts as one the tokenizer fails on, as Monaco's does once JavaScript's
/// stack runs out.
const MAX_NESTING: usize = 64;

/// So many steps in a row without moving along the line is a loop in the rules, which fails
/// the line (Monaco's rules have none).
const MAX_STALLED_STEPS: usize = 64;

/// Monaco's languages that read words otherwise than its default word pattern (see
/// `word_pattern`) or hand parts of a line to another language here, with their names
/// (`aliases` and the id, lower case) and MIME types (`text/x-<id>` when they declare none).
const LANGUAGES: &[(&str, &[&str], &[&str])] = &[
    ("typescript", &["typescript", "ts"], &["text/typescript"]),
    ("javascript", &["javascript", "js"], &["text/javascript"]),
    ("php", &["php"], &["application/x-php"]),
    ("java", &["java"], &["text/x-java-source", "text/x-java"]),
    ("kotlin", &["kotlin"], &["text/x-kotlin-source", "text/x-kotlin"]),
    ("csharp", &["c#", "csharp"], &["text/x-csharp"]),
    ("html", &["html", "htm", "xhtml"], &["text/html", "text/x-jshtm", "text/template", "text/ng-template"]),
    ("json", &["json"], &["application/json"]),
    ("css", &["css"], &["text/css"]),
    ("scss", &["sass", "scss"], &["text/x-scss", "text/scss"]),
    ("less", &["less"], &["text/x-less", "text/less"]),
    ("scala", &["scala", "sbt", "dotty"], &["text/x-scala-source", "text/x-scala", "text/x-sbt", "text/x-dotty"]),
    ("apex", &["apex"], &["text/x-apex-source", "text/x-apex"]),
    ("pascal", &["pascal", "pas"], &["text/x-pascal-source", "text/x-pascal"]),
    ("powershell", &["powershell", "ps", "ps1"], &["text/x-powershell"]),
    ("handlebars", &["handlebars", "hbs"], &["text/x-handlebars-template"]),
    ("liquid", &["liquid"], &["application/liquid"]),
    ("twig", &["twig"], &["text/x-twig"]),
    ("razor", &["razor"], &["text/x-cshtml"]),
    ("markdown", &["markdown"], &["text/x-markdown"]),
];

/// The language an embedded language's `name` (an id, alias or MIME type) is, as Monaco looks
/// it up (`getLanguageIdByLanguageName`, then `getLanguageIdByMimeType`); `""` for one not in
/// [`LANGUAGES`], which reads words with Monaco's default pattern (as Monaco's null language
/// does, for a name it does not know).
fn language_named(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    LANGUAGES
        .iter()
        .find(|(_, aliases, _)| aliases.contains(&lower.as_str()))
        .or_else(|| LANGUAGES.iter().find(|(_, _, mime_types)| mime_types.contains(&name)))
        .map_or("", |(language, _, _)| language)
}

/// What a rule does to the state stack.
#[derive(Debug, Clone, Copy)]
enum Next<S> {
    Stay,
    Push(S),
    Pop,
    Switch(S),
}

impl<S> Next<S> {
    fn map<T>(self, f: impl FnOnce(S) -> T) -> Next<T> {
        match self {
            Next::Stay => Next::Stay,
            Next::Push(state) => Next::Push(f(state)),
            Next::Pop => Next::Pop,
            Next::Switch(state) => Next::Switch(f(state)),
        }
    }
}

/// What a rule does to the embedded language (`nextEmbedded`).
#[derive(Debug, Clone, Copy)]
enum Embed<S> {
    Keep,
    /// Enter the language named (an id, alias or MIME type).
    Enter(S),
    /// Back to the tokenizer's own language (`@pop`).
    Leave,
}

impl<S> Embed<S> {
    fn map<T>(self, f: impl FnOnce(S) -> T) -> Embed<T> {
        match self {
            Embed::Keep => Embed::Keep,
            Embed::Enter(name) => Embed::Enter(f(name)),
            Embed::Leave => Embed::Leave,
        }
    }
}

/// A rule's action, with its states and names as written (`$1`, `$S2`) or substituted.
#[derive(Debug, Clone, Copy)]
struct Act<S> {
    /// `@rematch`: the match is left for the next state's rules.
    rematch: bool,
    next: Next<S>,
    embed: Embed<S>,
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Act(Act<&'static str>),
    /// Markdown's `>` that ends an HTML tag (its `cases`): `<style>` goes on in CSS,
    /// `<script>` in the language its `type` named or JavaScript, and other tags end.
    MarkdownTagEnd,
}

/// A rule as written: a JavaScript regular expression's source and its action, or a state
/// whose rules come in its place (`include`).
enum Def {
    Rule(String, Action),
    Include(&'static str),
}

fn act(next: Next<&'static str>) -> Act<&'static str> {
    Act { rematch: false, next, embed: Embed::Keep }
}

fn stay() -> Act<&'static str> {
    act(Next::Stay)
}

fn push(state: &'static str) -> Act<&'static str> {
    act(Next::Push(state))
}

fn pop() -> Act<&'static str> {
    act(Next::Pop)
}

fn switch(state: &'static str) -> Act<&'static str> {
    act(Next::Switch(state))
}

fn rematch(act: Act<&'static str>) -> Act<&'static str> {
    Act { rematch: true, ..act }
}

fn embed(act: Act<&'static str>, name: &'static str) -> Act<&'static str> {
    Act { embed: Embed::Enter(name), ..act }
}

fn leave(act: Act<&'static str>) -> Act<&'static str> {
    Act { embed: Embed::Leave, ..act }
}

fn rule(source: impl Into<String>, act: Act<&'static str>) -> Def {
    Def::Rule(source.into(), Action::Act(act))
}

fn include(state: &'static str) -> Def {
    Def::Include(state)
}

/// Whether `template` substitutes a group of the match (`$1`).
fn uses_groups(template: &str) -> bool {
    template.split('$').skip(1).any(|after| after.starts_with(|c: char| c.is_ascii_digit()))
}

/// A rule of a state, compiled.
struct Rule {
    /// Matches where the line has got to (`^(?:source)`).
    regex: Regex,
    /// Tried only at the start of the line (a source starting with `^`, without it).
    at_line_start: bool,
    action: Action,
    /// Whether the action substitutes the match's groups.
    uses_groups: bool,
    /// A rule leaving the embedded language: its source, searched for along the line.
    leaving: Option<Regex>,
}

/// A regular expression of a rule, which is known to be valid.
fn compile(source: &str) -> Regex {
    Regex::new(source).unwrap_or_else(|error| panic!("tokenizer rule {source}: {error}"))
}

/// The flags of a tokenizer's regular expressions.
fn flags(ignore_case: bool) -> &'static str {
    if ignore_case { "(?i)" } else { "" }
}

impl Rule {
    fn new(source: &str, action: Action, ignore_case: bool) -> Self {
        let at_line_start = source.starts_with('^');
        let body = source.strip_prefix('^').unwrap_or(source);
        let flags = flags(ignore_case);
        let (uses_groups, leaving) = match action {
            Action::Act(Act { next, embed, .. }) => {
                let groups = |template: &&str| uses_groups(template);
                let next_groups = matches!(next, Next::Push(state) | Next::Switch(state) if groups(&state));
                let embed_groups = matches!(embed, Embed::Enter(name) if groups(&name));
                let leaving = matches!(embed, Embed::Leave).then(|| compile(&format!("{flags}{body}")));
                (next_groups || embed_groups, leaving)
            }
            Action::MarkdownTagEnd => (false, None),
        };
        Rule { regex: compile(&format!("{flags}^(?:{body})")), at_line_start, action, uses_groups, leaving }
    }
}

/// The rules of `defs`, with those of the states they include in their place.
fn expand<'d>(by_name: &HashMap<&str, &'d [Def]>, defs: &'d [Def], rules: &mut Vec<(&'d str, Action)>) {
    for def in defs {
        match def {
            Def::Rule(source, action) => rules.push((source, *action)),
            Def::Include(state) => {
                let included = by_name.get(state).unwrap_or_else(|| panic!("no state {state} to include"));
                expand(by_name, included, rules);
            }
        }
    }
}

/// A state's rules.
struct StateRules {
    rules: Vec<Rule>,
    /// Matches where a rule could next match along the rest of a line, past its start: any
    /// rule but those tried only there, each without a leading `\b` (a rule is matched on the
    /// rest of the line alone, with nothing before it to make that fail). None when no rule
    /// is tried past the start of a line.
    next_candidate: Option<Regex>,
}

impl StateRules {
    fn new(defs: &[(&str, Action)], ignore_case: bool) -> Self {
        let rules = defs.iter().map(|(source, action)| Rule::new(source, *action, ignore_case)).collect();
        let candidates: Vec<String> = defs
            .iter()
            .filter(|(source, _)| !source.starts_with('^'))
            .map(|(source, _)| format!("(?:{})", source.strip_prefix(r"(?-u:\b)").unwrap_or(source)))
            .collect();
        let next_candidate =
            (!candidates.is_empty()).then(|| compile(&format!("{}{}", flags(ignore_case), candidates.join("|"))));
        StateRules { rules, next_candidate }
    }
}

/// A Monarch tokenizer, down to what decides the language of the text.
struct Lexer {
    /// Its own language.
    language: &'static str,
    /// The rules match in any case, and substituted groups and state arguments are lower case
    /// (`ignoreCase`, `fixCase`).
    ignore_case: bool,
    states: HashMap<&'static str, StateRules>,
}

impl Lexer {
    fn new(language: &'static str, ignore_case: bool, defs: Vec<(&'static str, Vec<Def>)>) -> Self {
        let by_name: HashMap<&str, &[Def]> = defs.iter().map(|(name, defs)| (*name, defs.as_slice())).collect();
        let states = defs
            .iter()
            .map(|(name, defs)| {
                let mut rules = Vec::new();
                expand(&by_name, defs, &mut rules);
                (*name, StateRules::new(&rules, ignore_case))
            })
            .collect();
        Lexer { language, ignore_case, states }
    }

    /// The rules of `state`, or of the state it names before its last dot, and so on
    /// (`findRules`).
    fn rules(&self, state: &str) -> Option<&StateRules> {
        let mut name = state;
        loop {
            if let Some(rules) = self.states.get(name) {
                return Some(rules);
            }
            name = &name[..name.rfind('.')?];
        }
    }

    /// `$n` (a group of the match) and `$Sn` (a part of the state's name, `$S0` the whole)
    /// substituted in `template` (`substituteMatches`).
    fn substitute(&self, template: &str, groups: Option<&Captures>, state: &str) -> String {
        let fix_case = |text: &str| if self.ignore_case { text.to_lowercase() } else { text.to_owned() };
        let mut result = String::new();
        let mut rest = template;
        while let Some(dollar) = rest.find('$') {
            result.push_str(&rest[..dollar]);
            let after = &rest[dollar + 1..];
            let (of_state, after_s) = match after.strip_prefix(['S', 's']) {
                Some(after_s) => (true, after_s),
                None => (false, after),
            };
            let digits = after_s.bytes().take(2).take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                // `$$` is a `$`; a `$` before anything else stays as it is.
                result.push('$');
                rest = after.strip_prefix('$').unwrap_or(after);
                continue;
            }
            let index = after_s.bytes().take(digits).fold(0, |index, digit| index * 10 + usize::from(digit - b'0'));
            let value = if of_state {
                match index {
                    0 => Some(state),
                    _ => state.split('.').nth(index - 1),
                }
            } else {
                groups.and_then(|groups| groups.get(index)).map(|group| group.as_str())
            };
            result.push_str(&value.map(fix_case).unwrap_or_default());
            rest = &after_s[digits..];
        }
        result.push_str(rest);
        result
    }

    /// What `rule`, matched at the start of `rest` in `state`, does.
    fn step(&self, rule: &Rule, rest: &str, state: &str) -> Act<String> {
        let act = match rule.action {
            Action::Act(act) => act,
            Action::MarkdownTagEnd => markdown_tag_end(state),
        };
        let groups = if rule.uses_groups { rule.regex.captures(rest) } else { None };
        let substitute = |template: &str| self.substitute(template, groups.as_ref(), state);
        Act { rematch: act.rematch, next: act.next.map(substitute), embed: act.embed.map(substitute) }
    }

    /// The languages of `line` from `state`, which moves on to the next line's
    /// (`tokenizeEncoded`). `delta` is where `line` starts in the whole line, `depth` how deep
    /// this tokenizer is nested.
    fn tokenize(&self, state: &mut LineState, line: &str, delta: usize, depth: usize, parts: &mut Parts) -> Tokenized {
        if depth > MAX_NESTING {
            return Err(Failed);
        }
        if line.len() >= MAX_TOKENIZATION_LINE_LENGTH && line.encode_utf16().count() >= MAX_TOKENIZATION_LINE_LENGTH {
            parts.push((delta, self.language));
            return Ok(());
        }
        match state.embedded {
            Some(_) => self.nested_tokenize(state, line, delta, depth, parts),
            None => self.own_tokenize(state, line, delta, depth, parts),
        }
    }

    /// `line` from inside the embedded language: it runs up to the first match of a rule of
    /// the state that leaves it, and the rest is this tokenizer's (`_nestedTokenize`).
    fn nested_tokenize(
        &self,
        state: &mut LineState,
        line: &str,
        delta: usize,
        depth: usize,
        parts: &mut Parts,
    ) -> Tokenized {
        let current = state.stack.last().ok_or(Failed)?;
        let rules = self.rules(current).ok_or(Failed)?;
        let mut leaving = rules.rules.iter().filter_map(|rule| Some((rule, rule.leaving.as_ref()?))).peekable();
        if leaving.peek().is_none() {
            return Err(Failed);
        }
        let end = leaving
            .filter_map(|(rule, regex)| {
                let found = regex.find(line)?.start();
                (found == 0 || !rule.at_line_start).then_some(found)
            })
            .min();
        let embedded = state.embedded.as_mut().ok_or(Failed)?;
        match end {
            None => embedded.tokenize(line, delta, depth, parts),
            Some(end) => {
                if end > 0 {
                    embedded.tokenize(&line[..end], delta, depth, parts)?;
                }
                self.own_tokenize(state, &line[end..], delta + end, depth, parts)
            }
        }
    }

    /// `line` in this tokenizer's own language, up to where it enters another (`_myTokenize`).
    fn own_tokenize(
        &self,
        state: &mut LineState,
        line: &str,
        delta: usize,
        depth: usize,
        parts: &mut Parts,
    ) -> Tokenized {
        parts.push((delta, self.language));
        let mut pos = 0;
        let mut first = true;
        let mut stalled = 0;
        while first || pos < line.len() {
            first = false;
            let rest = &line[pos..];
            let (step, matched) = {
                let current = state.stack.last().ok_or(Failed)?;
                let rules = self.rules(current).ok_or(Failed)?;
                let found = rules
                    .rules
                    .iter()
                    .filter(|rule| pos == 0 || !rule.at_line_start)
                    .find_map(|rule| Some((rule, rule.regex.find(rest)?.end())));
                let Some((rule, matched)) = found else {
                    // No rule matches: the character is skipped, in the state it is in, and so
                    // is each one after it up to where a rule could match.
                    pos += rest.chars().next().map_or(0, char::len_utf8);
                    let candidate = rules.next_candidate.as_ref().and_then(|candidate| candidate.find(&line[pos..]));
                    pos = candidate.map_or(line.len(), |candidate| pos + candidate.start());
                    continue;
                };
                (self.step(rule, rest, current), matched)
            };
            let moved = if step.rematch { 0 } else { matched };
            pos += moved;
            let entering = match step.embed {
                Embed::Keep => None,
                Embed::Enter(name) => match state.embedded {
                    Some(_) => return Err(Failed),
                    None => Some(name),
                },
                Embed::Leave => match state.embedded.take() {
                    Some(_) => None,
                    None => return Err(Failed),
                },
            };
            let changed = match step.next {
                Next::Stay => false,
                Next::Push(next) => {
                    if self.rules(&next).is_none() {
                        return Err(Failed);
                    }
                    state.stack.push(next);
                    true
                }
                Next::Pop if state.stack.len() > 1 => {
                    state.stack.pop();
                    true
                }
                Next::Pop => return Err(Failed),
                Next::Switch(next) => {
                    if self.rules(&next).is_none() {
                        return Err(Failed);
                    }
                    let top = state.stack.last_mut().ok_or(Failed)?;
                    let changed = *top != next;
                    *top = next;
                    changed
                }
            };
            if let Some(name) = entering {
                state.embedded = Some(Embedded::new(language_named(&name)));
                return if pos < line.len() {
                    self.nested_tokenize(state, &line[pos..], delta + pos, depth, parts)
                } else {
                    Ok(())
                };
            }
            if moved == 0 {
                // Monaco fails a line on a match of nothing that leaves the state as it was.
                stalled += 1;
                if (!changed && !line.is_empty()) || stalled > MAX_STALLED_STEPS {
                    return Err(Failed);
                }
            } else {
                stalled = 0;
            }
        }
        Ok(())
    }
}

/// The action of Markdown's `>` ending an HTML tag in `state` (`tag.<name>`, or
/// `tag.<name>.<type>` after a `type` attribute).
fn markdown_tag_end(state: &str) -> Act<&'static str> {
    let mut arguments = state.split('.').skip(1);
    match (arguments.next(), arguments.next()) {
        (Some("style"), _) => embed(switch("embeddedStyle"), "text/css"),
        (Some("script"), Some(type_name)) if !type_name.is_empty() => embed(switch("embeddedScript"), "$S3"),
        (Some("script"), _) => embed(switch("embeddedScript"), "text/javascript"),
        _ => pop(),
    }
}

/// Where each language starts on a line, in order, with the same start (an empty part) or
/// language repeated.
type Parts = Vec<(usize, &'static str)>;

/// The tokenizer failed on the line: Monaco reads it all as the file's language.
#[derive(Debug)]
struct Failed;

type Tokenized = Result<(), Failed>;

/// A tokenizer's state at the start of a line (`MonarchLineState`).
#[derive(Debug, Clone)]
struct LineState {
    /// Its states, the current one last.
    stack: Vec<String>,
    /// The language it has handed the text to, if any.
    embedded: Option<Embedded>,
}

impl LineState {
    fn new() -> Self {
        LineState { stack: vec!["root".to_owned()], embedded: None }
    }
}

/// An embedded language (`EmbeddedLanguageData`).
#[derive(Debug, Clone)]
struct Embedded {
    /// Its id ([`language_named`]).
    language: &'static str,
    /// Its own tokenizer and that one's state, for a language that embeds others in turn.
    tokenizer: Option<(&'static Lexer, Box<LineState>)>,
}

impl Embedded {
    fn new(language: &'static str) -> Self {
        Embedded { language, tokenizer: lexer(language).map(|lexer| (lexer, Box::new(LineState::new()))) }
    }

    /// `line` in this language (`nestedLanguageTokenize`).
    fn tokenize(&mut self, line: &str, delta: usize, depth: usize, parts: &mut Parts) -> Tokenized {
        match &mut self.tokenizer {
            Some((lexer, state)) => lexer.tokenize(state, line, delta, depth + 1, parts),
            None => {
                parts.push((delta, self.language));
                Ok(())
            }
        }
    }
}

impl std::fmt::Debug for Lexer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lexer").field("language", &self.language).finish_non_exhaustive()
    }
}

/// The tokenizer of Monaco's `language`, for one that hands parts of a line to another.
fn lexer(language: &str) -> Option<&'static Lexer> {
    match language {
        "html" => Some(&*HTML),
        "php" => Some(&*PHP),
        "markdown" => Some(&*MARKDOWN),
        _ => None,
    }
}

/// The parts of `line`, the line at byte `line_start` of `text`, a file of Monaco's
/// `language`, each with the language it is in, in order (neighbours differ in language;
/// none when the line is empty). Languages read here as their word patterns name them: those
/// of [`LANGUAGES`], and `""` for any other embedded one.
pub(super) fn line_parts<'l>(
    text: &str,
    line_start: usize,
    line: &str,
    language: &'l str,
) -> Vec<(Range<usize>, &'l str)> {
    let Some(lexer) = lexer(language) else {
        return if line.is_empty() { Vec::new() } else { vec![(0..line.len(), language)] };
    };
    let mut state = LineState::new();
    let mut parts = Parts::new();
    for (_, before) in super::lines(text).take_while(|(start, _)| *start < line_start) {
        parts.clear();
        tokenize_line(lexer, &mut state, before, &mut parts);
    }
    parts.clear();
    tokenize_line(lexer, &mut state, line, &mut parts);
    let mut merged: Vec<(Range<usize>, &'l str)> = Vec::new();
    for (index, &(start, part_language)) in parts.iter().enumerate() {
        let end = parts.get(index + 1).map_or(line.len(), |&(next, _)| next);
        if start >= end {
            continue;
        }
        match merged.last_mut() {
            Some((part, last)) if *last == part_language => part.end = end,
            Some(_) | None => merged.push((start..end, part_language)),
        }
    }
    merged
}

/// One line of the file, as Monaco's model tokenizes it (`safeTokenize`): a line the
/// tokenizer fails on is all the file's language, and leaves the state as it was.
fn tokenize_line(lexer: &'static Lexer, state: &mut LineState, line: &str, parts: &mut Parts) {
    let mut next = state.clone();
    match lexer.tokenize(&mut next, line, 0, 0, parts) {
        Ok(()) => *state = next,
        Err(Failed) => {
            parts.clear();
            parts.push((0, lexer.language));
        }
    }
}

/// The start of PHP's code (`<?php`, `<?=` or `<?`).
const PHP_OPEN: &str = r"<\?((php)|=)?";
/// PHP's string escapes (`escapes`).
const PHP_ESCAPES: &str = r#"\\(?:[abfnrtv\\"']|x[0-9A-Fa-f]{1,4}|u[0-9A-Fa-f]{4}|U[0-9A-Fa-f]{8})"#;
/// Markdown's escapes (`escapes`, of a `control` character).
const MARKDOWN_ESCAPES: &str = r"\\(?:[\\`*_\[\]{}()#+\-\.!])";

/// Monaco's HTML tokenizer (`basic-languages/html`), which Monaco also reads Vue and Svelte
/// files with: `<script>` is JavaScript, or the language of a `type` other than `module`, and
/// `<style>` CSS or its `type`'s.
static HTML: LazyLock<Lexer> = LazyLock::new(|| {
    Lexer::new(
        "html",
        true,
        vec![
            (
                "root",
                vec![
                    rule(r"<!DOCTYPE", push("doctype")),
                    rule(r"<!--", push("comment")),
                    rule(r"(<)((?:[0-9A-Za-z_\-]+:)?[0-9A-Za-z_\-]+)(\s*)(\/>)", stay()),
                    rule(r"(<)(script)", push("script")),
                    rule(r"(<)(style)", push("style")),
                    rule(r"(<)((?:[0-9A-Za-z_\-]+:)?[0-9A-Za-z_\-]+)", push("otherTag")),
                    rule(r"(<\/)((?:[0-9A-Za-z_\-]+:)?[0-9A-Za-z_\-]+)", push("otherTag")),
                    rule(r"<", stay()),
                    rule(r"[^<]+", stay()),
                ],
            ),
            ("doctype", vec![rule(r"[^>]+", stay()), rule(r">", pop())]),
            ("comment", vec![rule(r"-->", pop()), rule(r"[^\-]+", stay()), rule(r".", stay())]),
            (
                "otherTag",
                vec![
                    rule(r"\/?>", pop()),
                    rule(r#""([^"]*)""#, stay()),
                    rule(r"'([^']*)'", stay()),
                    rule(r"[0-9A-Za-z_\-]+", stay()),
                    rule(r"=", stay()),
                    rule(r"[ \t\r\n]+", stay()),
                ],
            ),
            (
                "script",
                vec![
                    rule(r"type", push("scriptAfterType")),
                    rule(r#""([^"]*)""#, stay()),
                    rule(r"'([^']*)'", stay()),
                    rule(r"[0-9A-Za-z_\-]+", stay()),
                    rule(r"=", stay()),
                    rule(r">", embed(push("scriptEmbedded"), "text/javascript")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"(<\/)(script\s*)(>)", pop()),
                ],
            ),
            (
                "scriptAfterType",
                vec![
                    rule(r"=", push("scriptAfterTypeEquals")),
                    rule(r">", embed(push("scriptEmbedded"), "text/javascript")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/script\s*>", rematch(pop())),
                ],
            ),
            (
                "scriptAfterTypeEquals",
                vec![
                    rule(r#""module""#, switch("scriptWithCustomType.text/javascript")),
                    rule(r"'module'", switch("scriptWithCustomType.text/javascript")),
                    rule(r#""([^"]*)""#, switch("scriptWithCustomType.$1")),
                    rule(r"'([^']*)'", switch("scriptWithCustomType.$1")),
                    rule(r">", embed(push("scriptEmbedded"), "text/javascript")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/script\s*>", rematch(pop())),
                ],
            ),
            (
                "scriptWithCustomType",
                vec![
                    rule(r">", embed(push("scriptEmbedded.$S2"), "$S2")),
                    rule(r#""([^"]*)""#, stay()),
                    rule(r"'([^']*)'", stay()),
                    rule(r"[0-9A-Za-z_\-]+", stay()),
                    rule(r"=", stay()),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/script\s*>", rematch(pop())),
                ],
            ),
            ("scriptEmbedded", vec![rule(r"<\/script", leave(rematch(pop()))), rule(r"[^<]+", stay())]),
            (
                "style",
                vec![
                    rule(r"type", push("styleAfterType")),
                    rule(r#""([^"]*)""#, stay()),
                    rule(r"'([^']*)'", stay()),
                    rule(r"[0-9A-Za-z_\-]+", stay()),
                    rule(r"=", stay()),
                    rule(r">", embed(push("styleEmbedded"), "text/css")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"(<\/)(style\s*)(>)", pop()),
                ],
            ),
            (
                "styleAfterType",
                vec![
                    rule(r"=", push("styleAfterTypeEquals")),
                    rule(r">", embed(push("styleEmbedded"), "text/css")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/style\s*>", rematch(pop())),
                ],
            ),
            (
                "styleAfterTypeEquals",
                vec![
                    rule(r#""([^"]*)""#, switch("styleWithCustomType.$1")),
                    rule(r"'([^']*)'", switch("styleWithCustomType.$1")),
                    rule(r">", embed(push("styleEmbedded"), "text/css")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/style\s*>", rematch(pop())),
                ],
            ),
            (
                "styleWithCustomType",
                vec![
                    rule(r">", embed(push("styleEmbedded.$S2"), "$S2")),
                    rule(r#""([^"]*)""#, stay()),
                    rule(r"'([^']*)'", stay()),
                    rule(r"[0-9A-Za-z_\-]+", stay()),
                    rule(r"=", stay()),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/style\s*>", rematch(pop())),
                ],
            ),
            ("styleEmbedded", vec![rule(r"<\/style", leave(rematch(pop()))), rule(r"[^<]+", stay())]),
        ],
    )
});

/// Monaco's PHP tokenizer (`basic-languages/php`): HTML (in the same case only) whose
/// `<script>` and `<style>` blocks are JavaScript and CSS, with PHP's code between `<?php`
/// (or `<?=`, `<?`) and `?>` anywhere in it, a string or block comment going on past `?>`.
static PHP: LazyLock<Lexer> = LazyLock::new(|| {
    let php_open = |state: &'static str| rule(PHP_OPEN, rematch(switch(state)));
    Lexer::new(
        "php",
        false,
        vec![
            (
                "root",
                vec![
                    php_open("phpInSimpleState.root"),
                    rule(r"<!DOCTYPE", push("doctype")),
                    rule(r"<!--", push("comment")),
                    rule(r"(<)([0-9A-Za-z_]+)(\/>)", stay()),
                    rule(r"(<)(script)", push("script")),
                    rule(r"(<)(style)", push("style")),
                    rule(r"(<)([:0-9A-Za-z_]+)", push("otherTag")),
                    rule(r"(<\/)([0-9A-Za-z_]+)", push("otherTag")),
                    rule(r"<", stay()),
                    rule(r"[^<]+", stay()),
                ],
            ),
            ("doctype", vec![php_open("phpInSimpleState.comment"), rule(r"[^>]+", stay()), rule(r">", pop())]),
            (
                "comment",
                vec![
                    php_open("phpInSimpleState.comment"),
                    rule(r"-->", pop()),
                    rule(r"[^\-]+", stay()),
                    rule(r".", stay()),
                ],
            ),
            (
                "otherTag",
                vec![
                    php_open("phpInSimpleState.otherTag"),
                    rule(r"\/?>", pop()),
                    rule(r#""([^"]*)""#, stay()),
                    rule(r"'([^']*)'", stay()),
                    rule(r"[0-9A-Za-z_\-]+", stay()),
                    rule(r"=", stay()),
                    rule(r"[ \t\r\n]+", stay()),
                ],
            ),
            (
                "script",
                vec![
                    php_open("phpInSimpleState.script"),
                    rule(r"type", push("scriptAfterType")),
                    rule(r#""([^"]*)""#, stay()),
                    rule(r"'([^']*)'", stay()),
                    rule(r"[0-9A-Za-z_\-]+", stay()),
                    rule(r"=", stay()),
                    rule(r">", embed(push("scriptEmbedded.text/javascript"), "text/javascript")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"(<\/)(script\s*)(>)", pop()),
                ],
            ),
            (
                "scriptAfterType",
                vec![
                    php_open("phpInSimpleState.scriptAfterType"),
                    rule(r"=", push("scriptAfterTypeEquals")),
                    rule(r">", embed(push("scriptEmbedded.text/javascript"), "text/javascript")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/script\s*>", rematch(pop())),
                ],
            ),
            (
                "scriptAfterTypeEquals",
                vec![
                    php_open("phpInSimpleState.scriptAfterTypeEquals"),
                    rule(r#""([^"]*)""#, switch("scriptWithCustomType.$1")),
                    rule(r"'([^']*)'", switch("scriptWithCustomType.$1")),
                    rule(r">", embed(push("scriptEmbedded.text/javascript"), "text/javascript")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/script\s*>", rematch(pop())),
                ],
            ),
            (
                "scriptWithCustomType",
                vec![
                    php_open("phpInSimpleState.scriptWithCustomType.$S2"),
                    rule(r">", embed(push("scriptEmbedded.$S2"), "$S2")),
                    rule(r#""([^"]*)""#, stay()),
                    rule(r"'([^']*)'", stay()),
                    rule(r"[0-9A-Za-z_\-]+", stay()),
                    rule(r"=", stay()),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/script\s*>", rematch(pop())),
                ],
            ),
            (
                "scriptEmbedded",
                vec![
                    rule(PHP_OPEN, leave(rematch(switch("phpInEmbeddedState.scriptEmbedded.$S2")))),
                    rule(r"<\/script", leave(rematch(pop()))),
                ],
            ),
            (
                "style",
                vec![
                    php_open("phpInSimpleState.style"),
                    rule(r"type", push("styleAfterType")),
                    rule(r#""([^"]*)""#, stay()),
                    rule(r"'([^']*)'", stay()),
                    rule(r"[0-9A-Za-z_\-]+", stay()),
                    rule(r"=", stay()),
                    rule(r">", embed(push("styleEmbedded.text/css"), "text/css")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"(<\/)(style\s*)(>)", pop()),
                ],
            ),
            (
                "styleAfterType",
                vec![
                    php_open("phpInSimpleState.styleAfterType"),
                    rule(r"=", push("styleAfterTypeEquals")),
                    rule(r">", embed(push("styleEmbedded.text/css"), "text/css")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/style\s*>", rematch(pop())),
                ],
            ),
            (
                "styleAfterTypeEquals",
                vec![
                    php_open("phpInSimpleState.styleAfterTypeEquals"),
                    rule(r#""([^"]*)""#, switch("styleWithCustomType.$1")),
                    rule(r"'([^']*)'", switch("styleWithCustomType.$1")),
                    rule(r">", embed(push("styleEmbedded.text/css"), "text/css")),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/style\s*>", rematch(pop())),
                ],
            ),
            (
                "styleWithCustomType",
                vec![
                    php_open("phpInSimpleState.styleWithCustomType.$S2"),
                    rule(r">", embed(push("styleEmbedded.$S2"), "$S2")),
                    rule(r#""([^"]*)""#, stay()),
                    rule(r"'([^']*)'", stay()),
                    rule(r"[0-9A-Za-z_\-]+", stay()),
                    rule(r"=", stay()),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"<\/style\s*>", rematch(pop())),
                ],
            ),
            (
                "styleEmbedded",
                vec![
                    rule(PHP_OPEN, leave(rematch(switch("phpInEmbeddedState.styleEmbedded.$S2")))),
                    rule(r"<\/style", leave(rematch(pop()))),
                ],
            ),
            ("phpInSimpleState", vec![rule(PHP_OPEN, stay()), rule(r"\?>", switch("$S2.$S3")), include("phpRoot")]),
            (
                "phpInEmbeddedState",
                vec![rule(PHP_OPEN, stay()), rule(r"\?>", embed(switch("$S2.$S3"), "$S3")), include("phpRoot")],
            ),
            (
                "phpRoot",
                vec![
                    rule(r"[a-zA-Z_][0-9A-Za-z_]*", stay()),
                    rule(r"[$a-zA-Z_][0-9A-Za-z_]*", stay()),
                    rule(r"[{}]", stay()),
                    rule(r"[\[\]]", stay()),
                    rule(r"[()]", stay()),
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r"(#|\/\/)$", stay()),
                    rule(r"(#|\/\/)", push("phpLineComment")),
                    rule(r"\/\*", push("phpComment")),
                    rule(r#"""#, push("phpDoubleQuoteString")),
                    rule(r"'", push("phpSingleQuoteString")),
                    rule(r"[+\-*%&|\^~!=<>/?;:.,@]", stay()),
                    rule(r"[0-9]*[0-9]+[eE]([\-+]?[0-9]+)?", stay()),
                    rule(r"[0-9]*\.[0-9]+([eE][\-+]?[0-9]+)?", stay()),
                    rule(r"0[xX][0-9a-fA-F']*[0-9a-fA-F]", stay()),
                    rule(r"0[0-7']*[0-7]", stay()),
                    rule(r"0[bB][0-1']*[0-1]", stay()),
                    rule(r"[0-9][0-9']*", stay()),
                    rule(r"[0-9]", stay()),
                ],
            ),
            ("phpComment", vec![rule(r"\*\/", pop()), rule(r"[^*]+", stay()), rule(r".", stay())]),
            (
                "phpLineComment",
                vec![
                    rule(r"\?>", rematch(pop())),
                    rule(r".$", pop()),
                    rule(r"[^?]+$", pop()),
                    rule(r"[^?]+", stay()),
                    rule(r".", stay()),
                ],
            ),
            (
                "phpDoubleQuoteString",
                vec![rule(r#"[^\\"]+"#, stay()), rule(PHP_ESCAPES, stay()), rule(r"\\.", stay()), rule(r#"""#, pop())],
            ),
            (
                "phpSingleQuoteString",
                vec![rule(r"[^\\']+", stay()), rule(PHP_ESCAPES, stay()), rule(r"\\.", stay()), rule(r"'", pop())],
            ),
        ],
    )
});

/// Monaco's Markdown tokenizer (`basic-languages/markdown`): a code fence opened with a
/// language (```` ```ts ````) is in that language until a line ending in ```` ``` ````, and
/// raw HTML's `<script>` and `<style>` blocks are JavaScript (or their `type`'s) and CSS.
static MARKDOWN: LazyLock<Lexer> = LazyLock::new(|| {
    let escapes = MARKDOWN_ESCAPES;
    Lexer::new(
        "markdown",
        false,
        vec![
            (
                "root",
                vec![
                    rule(r"^\s*\|", rematch(push("table_header"))),
                    rule(format!(r"^(\s{{0,3}})(#+)((?:[^\\#]|{escapes})+)((?:#+)?)"), stay()),
                    rule(r"^\s*(=+|\-+)\s*$", stay()),
                    rule(r"^\s*((\*[ ]?)+)\s*$", stay()),
                    rule(r"^\s*>+", stay()),
                    rule(r"^\s*([\*\-+:]|[0-9]+\.)\s", stay()),
                    rule(r"^(\t|[ ]{4})[^ ].*$", stay()),
                    rule(r"^\s*~~~\s*((?:[0-9A-Za-z_]|[\/\-#])+)?\s*$", push("codeblock")),
                    rule(r"^\s*```\s*((?:[0-9A-Za-z_]|[\/\-#])+).*$", embed(push("codeblockgh"), "$1")),
                    rule(r"^\s*```\s*$", push("codeblock")),
                    include("linecontent"),
                ],
            ),
            ("table_header", vec![include("table_common"), rule(r"[^\|]+", stay())]),
            ("table_body", vec![include("table_common"), include("linecontent")]),
            (
                "table_common",
                vec![
                    rule(r"\s*[\-:]+\s*", switch("table_body")),
                    rule(r"^\s*\|", stay()),
                    rule(r"^\s*[^\|]", rematch(pop())),
                    rule(r"^\s*$", rematch(pop())),
                    rule(r"\|", stay()),
                ],
            ),
            ("codeblock", vec![rule(r"^\s*~~~\s*$", pop()), rule(r"^\s*```\s*$", pop()), rule(r".*$", stay())]),
            ("codeblockgh", vec![rule(r"```\s*$", leave(pop())), rule(r"[^`]+", stay())]),
            (
                "linecontent",
                vec![
                    rule(r"&[0-9A-Za-z_]+;", stay()),
                    rule(escapes, stay()),
                    // `\b__([^\\_]|@escapes|_(?!_))+__\b`: a lone `_` takes what follows it.
                    rule(format!(r"(?-u:\b)__(?:[^\\_]|{escapes}|_(?:[^\\_]|{escapes}))+__(?-u:\b)"), stay()),
                    // `\*\*([^\\*]|@escapes|\*(?!\*))+\*\*`, likewise.
                    rule(format!(r"\*\*(?:[^\\*]|{escapes}|\*(?:[^\\*]|{escapes}))+\*\*"), stay()),
                    rule(r"(?-u:\b)_[^_]+_(?-u:\b)", stay()),
                    rule(format!(r"\*([^\\*]|{escapes})+\*"), stay()),
                    rule(format!(r"`([^\\`]|{escapes})+`"), stay()),
                    rule(r"\{+[^}]+\}+", stay()),
                    rule(format!(r"(!?\[)((?:[^\]\\]|{escapes})*)(\]\([^\)]+\))"), stay()),
                    rule(format!(r"(!?\[)((?:[^\]\\]|{escapes})*)(\])"), stay()),
                    include("html"),
                ],
            ),
            (
                "html",
                vec![
                    rule(r"<([0-9A-Za-z_]+)\/>", stay()),
                    rule(r"<([0-9A-Za-z_]+)(\-|[0-9A-Za-z_])*", push("tag.$1")),
                    rule(r"<\/([0-9A-Za-z_]+)(\-|[0-9A-Za-z_])*\s*>", stay()),
                    rule(r"<!--", push("comment")),
                ],
            ),
            (
                "comment",
                vec![rule(r"[^<\-]+", stay()), rule(r"-->", pop()), rule(r"<!--", stay()), rule(r"[<\-]", stay())],
            ),
            (
                "tag",
                vec![
                    rule(r"[ \t\r\n]+", stay()),
                    rule(r#"(type)(\s*=\s*)(")([^"]+)(")"#, switch("tag.$S2.$4")),
                    rule(r"(type)(\s*=\s*)(')([^']+)(')", switch("tag.$S2.$4")),
                    rule(r#"([0-9A-Za-z_]+)(\s*=\s*)("[^"]*"|'[^']*')"#, stay()),
                    rule(r"[0-9A-Za-z_]+", stay()),
                    rule(r"\/>", pop()),
                    Def::Rule(">".to_owned(), Action::MarkdownTagEnd),
                ],
            ),
            (
                "embeddedStyle",
                vec![rule(r"[^<]+", stay()), rule(r"<\/style\s*>", leave(rematch(pop()))), rule(r"<", stay())],
            ),
            (
                "embeddedScript",
                vec![rule(r"[^<]+", stay()), rule(r"<\/script\s*>", leave(rematch(pop()))), rule(r"<", stay())],
            ),
        ],
    )
});

#[cfg(test)]
mod tests {
    use super::*;

    /// The parts of the last line of `text`, a file of `language`, as (text, language).
    fn last_line(language: &str, text: &str) -> Vec<(String, String)> {
        let start = text.rfind('\n').map_or(0, |index| index + 1);
        let line = &text[start..];
        line_parts(text, start, line, language)
            .into_iter()
            .map(|(part, language)| (line[part].to_owned(), language.to_owned()))
            .collect()
    }

    fn parts(parts: &[(&str, &str)]) -> Vec<(String, String)> {
        parts.iter().map(|(text, language)| ((*text).to_owned(), (*language).to_owned())).collect()
    }

    #[test]
    fn every_rule_compiles() {
        for lexer in [&*HTML, &*PHP, &*MARKDOWN] {
            assert!(lexer.rules("root").is_some_and(|root| root.next_candidate.is_some()), "{}", lexer.language);
        }
    }

    #[test]
    fn languages_are_found_by_name_in_any_case_or_by_mime_type() {
        assert_eq!(language_named("TS"), "typescript");
        assert_eq!(language_named("C#"), "csharp");
        assert_eq!(language_named("text/css"), "css");
        assert_eq!(language_named("text/template"), "html");
        // MIME types are matched in their case only; unknown names have no language.
        assert_eq!(language_named("Text/CSS"), "");
        assert_eq!(language_named("text/x-template"), "");
        assert_eq!(language_named("python"), "");
    }

    #[test]
    fn substitutions_take_the_match_groups_and_the_state_names_parts() {
        let regex = Regex::new(r#"^(?:"([^"]*)")"#).unwrap();
        let groups = regex.captures(r#""Text/X""#);
        assert_eq!(MARKDOWN.substitute("a.$1", groups.as_ref(), "s"), "a.Text/X");
        assert_eq!(HTML.substitute("a.$1", groups.as_ref(), "s"), "a.text/x");
        assert_eq!(MARKDOWN.substitute("$S2.$S3", None, "tag.script.text/x"), "script.text/x");
        assert_eq!(MARKDOWN.substitute("$S0|$S4|$$|$x", None, "tag.script"), "tag.script||$|$x");
    }

    #[test]
    fn a_line_is_split_where_the_language_changes() {
        assert_eq!(
            last_line("html", "<p>a</p><script>let $x</script><style>.b{}</style>"),
            parts(&[
                ("<p>a</p><script>", "html"),
                ("let $x", "javascript"),
                ("</script><style>", "html"),
                (".b{}", "css"),
                ("</style>", "html"),
            ])
        );
        // A block goes on over lines, up to its end tag.
        assert_eq!(last_line("html", "<script>\nlet a;"), parts(&[("let a;", "javascript")]));
        assert_eq!(last_line("html", "<script>\n</script>"), parts(&[("</script>", "html")]));
        assert_eq!(last_line("rust", "fn a() {}"), parts(&[("fn a() {}", "rust")]));
        assert_eq!(last_line("html", "a\n"), parts(&[]));
    }

    #[test]
    fn markdown_code_fences_and_their_closing_line() {
        assert_eq!(last_line("markdown", "```ts\nlet $x = 1"), parts(&[("let $x = 1", "typescript")]));
        assert_eq!(last_line("markdown", "```ts\nend ```"), parts(&[("end ", "typescript"), ("```", "markdown")]));
        assert_eq!(last_line("markdown", "```ts\n```\n$x"), parts(&[("$x", "markdown")]));
        // A fence of an unknown language, or none.
        assert_eq!(last_line("markdown", "```mermaid\na-b"), parts(&[("a-b", "")]));
        assert_eq!(last_line("markdown", "```\na-b"), parts(&[("a-b", "markdown")]));
    }

    #[test]
    fn a_failing_line_is_all_the_files_language_and_keeps_the_state() {
        let mut state = LineState::new();
        state.stack = vec!["noSuchState".to_owned()];
        let mut parts = Parts::new();
        tokenize_line(&HTML, &mut state, "<script>", &mut parts);
        assert_eq!(parts, [(0, "html")]);
        assert_eq!(state.stack, ["noSuchState"]);
    }
}
