//! Keys, key presses and shortcuts, plus the Electron-accelerator parser and the GPUI
//! keybinding converter (replaces `keybindingConverter.ts` + tinykeys' `parseKeybinding`).
//!
//! Keys are held in GPUI's canonical form so presses and parsed settings compare directly:
//! characters are lowercase, and Shift is kept only on ASCII letters and named keys. Any
//! other character already encodes Shift (GPUI reports `!`, not `shift-1`), so Shift is
//! folded into it.

use std::borrow::Cow;

/// The host's keyboard conventions: what `CommandOrControl` means and how GPUI spells the
/// platform modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    /// Every other host. TreeTerm needs Unix sockets, so in practice Linux.
    Linux,
}

impl Os {
    pub const fn host() -> Self {
        if cfg!(target_os = "macos") { Os::MacOs } else { Os::Linux }
    }
}

/// Modifier state, mirroring GPUI's `Modifiers`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    /// Cmd on macOS, Super elsewhere (GPUI's `platform`).
    pub platform: bool,
    /// The Fn key (GPUI's `function`).
    pub function: bool,
}

impl Modifiers {
    pub const NONE: Self =
        Self { control: false, alt: false, shift: false, platform: false, function: false };
}

/// A modifier key pressed on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierKey {
    Control,
    Alt,
    Shift,
    Platform,
    Function,
}

impl ModifierKey {
    const ALL: [ModifierKey; 5] = [
        ModifierKey::Control,
        ModifierKey::Alt,
        ModifierKey::Shift,
        ModifierKey::Platform,
        ModifierKey::Function,
    ];

    /// GPUI's `Keystroke::key` for the modifier pressed alone.
    fn gpui_name(self) -> &'static str {
        match self {
            ModifierKey::Control => "control",
            ModifierKey::Alt => "alt",
            ModifierKey::Shift => "shift",
            ModifierKey::Platform => "platform",
            ModifierKey::Function => "function",
        }
    }

    fn label(self, os: Os) -> &'static str {
        match self {
            ModifierKey::Control => "Ctrl",
            ModifierKey::Alt => "Alt",
            ModifierKey::Shift => "Shift",
            ModifierKey::Platform => match os {
                Os::MacOs => "Cmd",
                Os::Linux => "Super",
            },
            ModifierKey::Function => "Fn",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// A character key, lowercase (`a`, `1`, `,`, `!`, `ö`). Never a space or control char.
    Char(char),
    Space,
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    /// `F1` ..= `F35`.
    F(u8),
    /// A modifier pressed on its own. Only reported by the UI, never bound.
    Modifier(ModifierKey),
    /// A key GPUI names that settings cannot spell (media keys, ...). Never bound.
    Other(String),
}

/// GPUI names the function keys up to F35.
const MAX_FUNCTION_KEY: u8 = 35;

/// The keys GPUI reports by name, besides function and modifier keys.
const NAMED_KEYS: [Key; 15] = [
    Key::Space,
    Key::Enter,
    Key::Tab,
    Key::Escape,
    Key::Backspace,
    Key::Delete,
    Key::Insert,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
];

/// Spellings settings may use for keys beyond the GPUI names: Electron accelerator names and
/// DOM `KeyboardEvent.key` values (the Electron settings dialog records the latter).
const KEY_ALIASES: [(&str, Key); 7] = [
    ("return", Key::Enter),
    ("esc", Key::Escape),
    ("arrowup", Key::Up),
    ("arrowdown", Key::Down),
    ("arrowleft", Key::Left),
    ("arrowright", Key::Right),
    ("plus", Key::Char('+')),
];

/// `c` lowercased when that is a single character.
fn lowercase(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

fn single_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// `f1` ..= `f35` (case-insensitive) as a function key number.
fn function_key(name: &str) -> Option<u8> {
    let digits = name.strip_prefix(['f', 'F'])?;
    if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u8>().ok().filter(|n| (1..=MAX_FUNCTION_KEY).contains(n))
}

impl Key {
    /// The key GPUI reports as `Keystroke::key`. Names this module has no variant for become
    /// [`Key::Other`].
    pub fn from_gpui(name: &str) -> Key {
        if let Some(key) = NAMED_KEYS.iter().find(|key| key.gpui_name() == name) {
            return key.clone();
        }
        if let Some(modifier) = ModifierKey::ALL.into_iter().find(|m| m.gpui_name() == name) {
            return Key::Modifier(modifier);
        }
        if let Some(n) = function_key(name) {
            return Key::F(n);
        }
        match single_char(name) {
            Some(' ') => Key::Space,
            Some(c) if !c.is_control() => Key::Char(lowercase(c)),
            _ => Key::Other(name.to_owned()),
        }
    }

    /// A key as settings spell it: a single character (`c`, `,`, `B`), an Electron
    /// accelerator key name (`Enter`, `Esc`, `Up`, `Plus`, `F5`) or a DOM key value
    /// (`ArrowUp`, `Escape`, ` `). Names are case-insensitive.
    pub fn parse(name: &str) -> Result<Key, ParseError> {
        let unknown = || ParseError::UnknownKey { binding: name.to_owned(), key: name.to_owned() };
        if name.is_empty() {
            return Err(ParseError::Empty);
        }
        if let Some(c) = single_char(name) {
            return match c {
                ' ' => Ok(Key::Space),
                c if c.is_control() => Err(unknown()),
                c => Ok(Key::Char(lowercase(c))),
            };
        }
        let lower = name.to_ascii_lowercase();
        if let Some(key) = NAMED_KEYS.iter().find(|key| key.gpui_name() == lower) {
            return Ok(key.clone());
        }
        if let Some((_, key)) = KEY_ALIASES.iter().find(|(alias, _)| *alias == lower) {
            return Ok(key.clone());
        }
        if let Some(n) = function_key(&lower) {
            return Ok(Key::F(n));
        }
        match modifier_name(&lower) {
            Some(_) => Err(ParseError::ModifierAsKey { binding: name.to_owned(), key: name.to_owned() }),
            None => Err(unknown()),
        }
    }

    /// GPUI's name for the key, as used in keystrokes and keybinding strings.
    pub fn gpui_name(&self) -> Cow<'_, str> {
        Cow::Borrowed(match self {
            Key::Char(c) => return Cow::Owned(c.to_string()),
            Key::Space => "space",
            Key::Enter => "enter",
            Key::Tab => "tab",
            Key::Escape => "escape",
            Key::Backspace => "backspace",
            Key::Delete => "delete",
            Key::Insert => "insert",
            Key::Home => "home",
            Key::End => "end",
            Key::PageUp => "pageup",
            Key::PageDown => "pagedown",
            Key::Up => "up",
            Key::Down => "down",
            Key::Left => "left",
            Key::Right => "right",
            Key::F(n) => return Cow::Owned(format!("f{n}")),
            Key::Modifier(modifier) => modifier.gpui_name(),
            Key::Other(name) => name,
        })
    }

    /// How the overlay and settings show the key: `B`, `,`, `Enter`, `F5`.
    pub fn label(&self, os: Os) -> Cow<'_, str> {
        Cow::Borrowed(match self {
            Key::Char(c) => return Cow::Owned(c.to_uppercase().collect()),
            Key::Space => "Space",
            Key::Enter => "Enter",
            Key::Tab => "Tab",
            Key::Escape => "Escape",
            Key::Backspace => "Backspace",
            Key::Delete => "Delete",
            Key::Insert => "Insert",
            Key::Home => "Home",
            Key::End => "End",
            Key::PageUp => "PageUp",
            Key::PageDown => "PageDown",
            Key::Up => "Up",
            Key::Down => "Down",
            Key::Left => "Left",
            Key::Right => "Right",
            Key::F(n) => return Cow::Owned(format!("F{n}")),
            Key::Modifier(modifier) => modifier.label(os),
            Key::Other(name) => name,
        })
    }
}

/// Canonical form shared by presses and shortcuts: lowercase characters, Shift only on ASCII
/// letters (see the module docs).
fn canonical(key: Key, modifiers: Modifiers) -> (Key, Modifiers) {
    match key {
        Key::Char(c) => (
            Key::Char(lowercase(c)),
            Modifiers { shift: modifiers.shift && c.is_ascii_alphabetic(), ..modifiers },
        ),
        other => (other, modifiers),
    }
}

/// The character a press types, as far as shortcut matching cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Typed {
    /// No text (Ctrl/Cmd held, a named key, an IME sequence of several characters).
    Nothing,
    /// Lowercase.
    Char(char),
}

/// One keystroke as the UI reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    key: Key,
    modifiers: Modifiers,
    typed: Typed,
}

impl KeyPress {
    /// A press of `key` that types nothing (as GPUI reports keys with Ctrl or Cmd held).
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        let (key, modifiers) = canonical(key, modifiers);
        Self { key, modifiers, typed: Typed::Nothing }
    }

    /// The same press typing `c` (GPUI's `key_char`, e.g. `ß` for Option-s on macOS).
    pub fn typing(self, c: char) -> Self {
        let typed =
            if c.is_control() || c.is_whitespace() { Typed::Nothing } else { Typed::Char(lowercase(c)) };
        Self { typed, ..self }
    }

    /// From a GPUI `Keystroke`: `key`, `key_char` and its modifiers.
    pub fn from_gpui(key: &str, key_char: Option<&str>, modifiers: Modifiers) -> Self {
        let press = Self::new(Key::from_gpui(key), modifiers);
        match key_char.and_then(single_char) {
            Some(c) => press.typing(c),
            None => press,
        }
    }

    pub fn key(&self) -> &Key {
        &self.key
    }

    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Whether the press is `key` or types it, whatever the modifiers: how the Electron app
    /// compared `KeyboardEvent.key` case-insensitively for the keys after the prefix.
    pub fn is_or_types(&self, key: &Key) -> bool {
        match (key, self.typed) {
            _ if self.key == *key => true,
            (Key::Char(c), Typed::Char(typed)) => *c == typed,
            _ => false,
        }
    }
}

/// A key with exact modifiers, e.g. the prefix key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    key: Key,
    modifiers: Modifiers,
}

impl Shortcut {
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        let (key, modifiers) = canonical(key, modifiers);
        Self { key, modifiers }
    }

    /// Parse an Electron accelerator as stored in `settings.json`: `Control+B`,
    /// `CommandOrControl+Shift+K`, `Shift+Alt+X`, `Control+Plus` / `Control++`, `Alt+F5`.
    /// Modifiers and key names are case-insensitive; `CommandOrControl` resolves for `os`.
    pub fn parse(accelerator: &str, os: Os) -> Result<Self, ParseError> {
        if accelerator.is_empty() {
            return Err(ParseError::Empty);
        }
        let empty_part = || ParseError::EmptyPart { binding: accelerator.to_owned() };
        // `+` itself as the key: `+` alone or after the modifiers (`Control++`).
        let (modifier_part, key_part) = if accelerator == "+" {
            (None, "+")
        } else if let Some(modifiers) = accelerator.strip_suffix("++") {
            (Some(modifiers), "+")
        } else {
            match accelerator.rsplit_once('+') {
                Some((modifiers, key)) => (Some(modifiers), key),
                None => (None, accelerator),
            }
        };
        if key_part.is_empty() {
            return Err(empty_part());
        }
        let mut modifiers = Modifiers::NONE;
        for part in modifier_part.map(|m| m.split('+')).into_iter().flatten() {
            if part.is_empty() {
                return Err(empty_part());
            }
            match modifier_name(&part.to_ascii_lowercase()) {
                Some(ModifierName::Control) => modifiers.control = true,
                Some(ModifierName::Alt) => modifiers.alt = true,
                Some(ModifierName::Shift) => modifiers.shift = true,
                Some(ModifierName::Platform) => modifiers.platform = true,
                Some(ModifierName::CommandOrControl) => match os {
                    Os::MacOs => modifiers.platform = true,
                    Os::Linux => modifiers.control = true,
                },
                Some(ModifierName::AltGr) => {
                    return Err(ParseError::UnsupportedModifier {
                        binding: accelerator.to_owned(),
                        modifier: part.to_owned(),
                    });
                }
                None => {
                    return Err(ParseError::UnknownModifier {
                        binding: accelerator.to_owned(),
                        modifier: part.to_owned(),
                    });
                }
            }
        }
        let key = Key::parse(key_part).map_err(|error| error.in_binding(accelerator))?;
        Ok(Self::new(key, modifiers))
    }

    pub fn key(&self) -> &Key {
        &self.key
    }

    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Whether `press` triggers this shortcut, by GPUI's rule (`Keystroke::should_match`):
    /// the key with exactly these modifiers, or the character the press types with only its
    /// Ctrl/Cmd modifiers (Option-composed characters, non-QWERTY layouts).
    pub fn matches(&self, press: &KeyPress) -> bool {
        if let Typed::Char(typed) = press.typed
            && press.key != Key::Char(typed)
        {
            let ime_modifiers = Modifiers {
                control: press.modifiers.control,
                platform: press.modifiers.platform,
                ..Modifiers::NONE
            };
            if self.key == Key::Char(typed) && self.modifiers == ime_modifiers {
                return true;
            }
        }
        self.modifiers == press.modifiers && self.key == press.key
    }

    /// GPUI keybinding syntax (`ctrl-b`, `cmd-shift-k`, `alt-f5`), for registering a GPUI
    /// `KeyBinding`. Modifiers come in GPUI's own order.
    pub fn to_gpui(&self, os: Os) -> String {
        let m = self.modifiers;
        let platform = match os {
            Os::MacOs => "cmd-",
            Os::Linux => "super-",
        };
        let prefixes = [
            (m.function, "fn-"),
            (m.control, "ctrl-"),
            (m.alt, "alt-"),
            (m.platform, platform),
            (m.shift, "shift-"),
        ];
        let mut out: String = prefixes.iter().filter(|(on, _)| *on).map(|(_, p)| *p).collect();
        out.push_str(&self.key.gpui_name());
        out
    }

    /// How the overlay shows the shortcut: `Ctrl + B`, `Cmd + Shift + K`.
    pub fn label(&self, os: Os) -> String {
        let m = self.modifiers;
        let modifiers = [
            (m.function, ModifierKey::Function),
            (m.control, ModifierKey::Control),
            (m.platform, ModifierKey::Platform),
            (m.alt, ModifierKey::Alt),
            (m.shift, ModifierKey::Shift),
        ];
        let mut parts: Vec<Cow<'_, str>> = modifiers
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, modifier)| Cow::Borrowed(modifier.label(os)))
            .collect();
        parts.push(self.key.label(os));
        parts.join(" + ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModifierName {
    Control,
    Alt,
    Shift,
    Platform,
    CommandOrControl,
    AltGr,
}

/// Electron accelerator modifier names, lowercased.
fn modifier_name(lower: &str) -> Option<ModifierName> {
    match lower {
        "control" | "ctrl" => Some(ModifierName::Control),
        "alt" | "option" => Some(ModifierName::Alt),
        "shift" => Some(ModifierName::Shift),
        "command" | "cmd" | "super" | "meta" => Some(ModifierName::Platform),
        "commandorcontrol" | "cmdorctrl" => Some(ModifierName::CommandOrControl),
        "altgr" => Some(ModifierName::AltGr),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("empty key binding")]
    Empty,
    #[error("key binding {binding:?} has an empty part")]
    EmptyPart { binding: String },
    #[error("key binding {binding:?}: unknown modifier {modifier:?}")]
    UnknownModifier { binding: String, modifier: String },
    #[error("key binding {binding:?}: modifier {modifier:?} is not supported")]
    UnsupportedModifier { binding: String, modifier: String },
    #[error("key binding {binding:?}: {key:?} is a modifier, not a key")]
    ModifierAsKey { binding: String, key: String },
    #[error("key binding {binding:?}: unknown key {key:?}")]
    UnknownKey { binding: String, key: String },
}

impl ParseError {
    /// The same error reported against the whole binding a key part came from.
    fn in_binding(self, binding: &str) -> Self {
        let binding = binding.to_owned();
        match self {
            ParseError::Empty | ParseError::EmptyPart { .. } => ParseError::EmptyPart { binding },
            ParseError::UnknownModifier { modifier, .. } => ParseError::UnknownModifier { binding, modifier },
            ParseError::UnsupportedModifier { modifier, .. } => {
                ParseError::UnsupportedModifier { binding, modifier }
            }
            ParseError::ModifierAsKey { key, .. } => ParseError::ModifierAsKey { binding, key },
            ParseError::UnknownKey { key, .. } => ParseError::UnknownKey { binding, key },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: Modifiers = Modifiers { control: true, ..Modifiers::NONE };

    fn parse(accelerator: &str, os: Os) -> Shortcut {
        Shortcut::parse(accelerator, os).unwrap()
    }

    fn gpui(accelerator: &str, os: Os) -> String {
        parse(accelerator, os).to_gpui(os)
    }

    /// Port of `keybindingConverter.test.ts`: the TS converted to tinykeys syntax, this
    /// converts to GPUI's. Same cases, same names.
    mod convert_direct_keybinding {
        use super::*;

        #[test]
        fn converts_command_or_control_to_mod() {
            assert_eq!(gpui("CommandOrControl+B", Os::MacOs), "cmd-b");
            assert_eq!(gpui("CommandOrControl+B", Os::Linux), "ctrl-b");
        }

        #[test]
        fn lowercases_the_last_key_part_when_it_is_a_single_character() {
            assert_eq!(gpui("Ctrl+A", Os::Linux), "ctrl-a");
        }

        #[test]
        fn does_not_lowercase_non_single_character_final_parts() {
            // Named keys map to GPUI's (lowercase) names instead of being mangled.
            assert_eq!(parse("Ctrl+Enter", Os::Linux).key(), &Key::Enter);
            assert_eq!(gpui("Ctrl+Enter", Os::Linux), "ctrl-enter");
        }

        #[test]
        fn handles_binding_with_no_modifier() {
            assert_eq!(gpui("B", Os::MacOs), "b");
        }

        #[test]
        fn handles_multi_modifier_bindings() {
            assert_eq!(gpui("CommandOrControl+Shift+K", Os::MacOs), "cmd-shift-k");
            assert_eq!(gpui("CommandOrControl+Shift+K", Os::Linux), "ctrl-shift-k");
        }

        #[test]
        fn returns_the_binding_unchanged_when_it_is_already_lowercase() {
            assert_eq!(gpui("Ctrl+b", Os::Linux), "ctrl-b");
        }

        #[test]
        fn handles_escape_key_multi_char_no_transform() {
            assert_eq!(gpui("Ctrl+Escape", Os::Linux), "ctrl-escape");
        }

        #[test]
        fn handles_uppercase_single_letter_key() {
            assert_eq!(gpui("Alt+Z", Os::Linux), "alt-z");
        }
    }

    #[test]
    fn modifiers_come_out_in_gpui_order_whatever_the_input_order() {
        assert_eq!(gpui("Shift+Alt+X", Os::Linux), "alt-shift-x");
        assert_eq!(gpui("Shift+Super+Alt+Control+X", Os::Linux), "ctrl-alt-super-shift-x");
        assert_eq!(gpui("Shift+Cmd+Alt+Control+X", Os::MacOs), "ctrl-alt-cmd-shift-x");
    }

    #[test]
    fn every_electron_modifier_alias_is_understood() {
        let platform = Modifiers { platform: true, ..Modifiers::NONE };
        let alt = Modifiers { alt: true, ..Modifiers::NONE };
        for (accelerator, os, modifiers) in [
            ("Control+B", Os::MacOs, CTRL),
            ("ctrl+B", Os::MacOs, CTRL),
            ("CmdOrCtrl+B", Os::Linux, CTRL),
            ("CmdOrCtrl+B", Os::MacOs, platform),
            ("Command+B", Os::MacOs, platform),
            ("Cmd+B", Os::Linux, platform),
            ("Super+B", Os::Linux, platform),
            ("Meta+B", Os::MacOs, platform),
            ("Option+B", Os::MacOs, alt),
            ("ALT+B", Os::Linux, alt),
        ] {
            assert_eq!(parse(accelerator, os).modifiers(), modifiers, "{accelerator}");
        }
    }

    #[test]
    fn named_keys_accept_electron_and_dom_spellings() {
        for (accelerator, gpui_key) in [
            ("Control+Up", "ctrl-up"),
            ("Control+ArrowUp", "ctrl-up"),
            ("Control+ArrowDown", "ctrl-down"),
            ("Control+ArrowLeft", "ctrl-left"),
            ("Control+ArrowRight", "ctrl-right"),
            ("Control+Return", "ctrl-enter"),
            ("Control+Esc", "ctrl-escape"),
            ("Control+PageDown", "ctrl-pagedown"),
            ("Control+Space", "ctrl-space"),
            ("Control+ ", "ctrl-space"),
            ("Control+Tab", "ctrl-tab"),
            ("Control+Backspace", "ctrl-backspace"),
            ("Control+Delete", "ctrl-delete"),
            ("Alt+F5", "alt-f5"),
            ("f35", "f35"),
            ("Control+Plus", "ctrl-+"),
            ("Control++", "ctrl-+"),
            ("+", "+"),
            ("Control+-", "ctrl--"),
            ("Control+,", "ctrl-,"),
        ] {
            assert_eq!(gpui(accelerator, Os::Linux), gpui_key, "{accelerator}");
        }
    }

    #[test]
    fn shift_is_folded_into_characters_that_already_encode_it() {
        // The settings dialog records `KeyboardEvent.key`, so Shift+1 is stored as `Shift+!`;
        // GPUI reports that press as `!` without Shift.
        assert_eq!(gpui("CommandOrControl+Shift+!", Os::MacOs), "cmd-!");
        assert_eq!(gpui("CommandOrControl+Shift+B", Os::MacOs), "cmd-shift-b");
        assert_eq!(gpui("Shift+Enter", Os::MacOs), "shift-enter");
    }

    #[test]
    fn malformed_bindings_are_errors() {
        let binding = |s: &str| s.to_owned();
        for (accelerator, error) in [
            ("", ParseError::Empty),
            ("Control+", ParseError::EmptyPart { binding: binding("Control+") }),
            ("+B", ParseError::EmptyPart { binding: binding("+B") }),
            ("Control++B", ParseError::EmptyPart { binding: binding("Control++B") }),
            ("++", ParseError::EmptyPart { binding: binding("++") }),
            (
                "Hyper+B",
                ParseError::UnknownModifier { binding: binding("Hyper+B"), modifier: binding("Hyper") },
            ),
            (
                "AltGr+B",
                ParseError::UnsupportedModifier { binding: binding("AltGr+B"), modifier: binding("AltGr") },
            ),
            (
                "Control+Shift",
                ParseError::ModifierAsKey { binding: binding("Control+Shift"), key: binding("Shift") },
            ),
            (
                "Control+VolumeUp",
                ParseError::UnknownKey { binding: binding("Control+VolumeUp"), key: binding("VolumeUp") },
            ),
            ("F36", ParseError::UnknownKey { binding: binding("F36"), key: binding("F36") }),
            ("Control+\t", ParseError::UnknownKey { binding: binding("Control+\t"), key: binding("\t") }),
        ] {
            assert_eq!(Shortcut::parse(accelerator, Os::Linux), Err(error), "{accelerator:?}");
        }
    }

    #[test]
    fn function_keys_need_a_number_in_range() {
        assert_eq!(Key::parse("F1"), Ok(Key::F(1)));
        assert_eq!(Key::parse("f12"), Ok(Key::F(12)));
        for name in ["F0", "F01", "F36", "F1a", "Fx"] {
            assert!(matches!(Key::parse(name), Err(ParseError::UnknownKey { .. })), "{name}");
        }
        assert_eq!(Key::parse("f"), Ok(Key::Char('f')));
    }

    #[test]
    fn labels_follow_the_overlay_wording() {
        assert_eq!(parse("Control+B", Os::MacOs).label(Os::MacOs), "Ctrl + B");
        assert_eq!(parse("CommandOrControl+B", Os::MacOs).label(Os::MacOs), "Cmd + B");
        assert_eq!(parse("CommandOrControl+B", Os::Linux).label(Os::Linux), "Ctrl + B");
        assert_eq!(parse("Meta+Shift+Alt+F5", Os::Linux).label(Os::Linux), "Super + Alt + Shift + F5");
        assert_eq!(parse("Alt+PageUp", Os::Linux).label(Os::Linux), "Alt + PageUp");
        assert_eq!(Key::Modifier(ModifierKey::Platform).label(Os::MacOs), "Cmd");
        assert_eq!(Key::Other("menu".to_owned()).label(Os::Linux), "menu");
    }

    #[test]
    fn gpui_key_names_round_trip() {
        let names = [
            "a",
            "1",
            ",",
            "!",
            "space",
            "enter",
            "tab",
            "escape",
            "backspace",
            "delete",
            "insert",
            "home",
            "end",
            "pageup",
            "pagedown",
            "up",
            "down",
            "left",
            "right",
            "f1",
            "f35",
            "shift",
            "control",
            "alt",
            "platform",
            "function",
            "menu",
        ];
        for name in names {
            assert_eq!(Key::from_gpui(name).gpui_name(), name);
        }
        assert_eq!(Key::from_gpui("A"), Key::Char('a'));
        assert_eq!(Key::from_gpui(" "), Key::Space);
        assert_eq!(Key::from_gpui("f36"), Key::Other("f36".to_owned()));
        assert_eq!(Key::from_gpui(""), Key::Other(String::new()));
    }

    /// Port of the `matchesKeybinding` table in `keybinding.test.ts`.
    mod matches_keybinding {
        use super::*;

        fn mods(control: bool, shift: bool, alt: bool, meta: bool) -> Modifiers {
            Modifiers { control, shift, alt, platform: meta, function: false }
        }

        fn case(binding: Modifiers, key: &str, event: Modifiers, event_key: &str) -> bool {
            let shortcut = Shortcut::new(Key::from_gpui(key), binding);
            shortcut.matches(&KeyPress::from_gpui(event_key, None, event))
        }

        const NO: Modifiers = Modifiers::NONE;

        #[test]
        fn plain_key() {
            assert!(case(NO, "t", NO, "t"));
        }

        #[test]
        fn different_key() {
            assert!(!case(NO, "t", NO, "x"));
        }

        #[test]
        fn meta_modifier() {
            assert!(case(mods(false, false, false, true), "b", mods(false, false, false, true), "b"));
        }

        #[test]
        fn meta_required_but_not_pressed() {
            assert!(!case(mods(false, false, false, true), "b", NO, "b"));
        }

        #[test]
        fn extra_modifier_pressed() {
            assert!(!case(mods(false, false, false, true), "b", mods(false, true, false, true), "b"));
        }

        #[test]
        fn control_modifier() {
            assert!(case(CTRL, "a", CTRL, "a"));
        }

        #[test]
        fn multiple_modifiers() {
            assert!(case(mods(true, true, false, false), "k", mods(true, true, false, false), "k"));
        }

        #[test]
        fn missing_one_modifier() {
            assert!(!case(mods(true, true, false, false), "k", CTRL, "k"));
        }

        #[test]
        fn case_insensitive() {
            assert!(case(NO, "t", NO, "T"));
        }

        #[test]
        fn alt_modifier() {
            assert!(case(mods(false, false, true, false), "x", mods(false, false, true, false), "x"));
        }

        #[test]
        fn all_four_modifiers() {
            let all = mods(true, true, true, true);
            assert!(case(all, "z", all, "z"));
        }
    }

    #[test]
    fn a_typed_character_matches_like_gpui_does() {
        let alt = Modifiers { alt: true, ..Modifiers::NONE };
        // Option-s types ß on macOS: it triggers both `alt-s` and a bare `ß` binding.
        let option_s = KeyPress::from_gpui("s", Some("ß"), alt);
        assert!(Shortcut::new(Key::Char('s'), alt).matches(&option_s));
        assert!(Shortcut::new(Key::Char('ß'), Modifiers::NONE).matches(&option_s));
        assert!(!Shortcut::new(Key::Char('ß'), alt).matches(&option_s));
        // Shift+c types C: that is just `shift-c`, not a bare `c`.
        let shift = Modifiers { shift: true, ..Modifiers::NONE };
        let shift_c = KeyPress::from_gpui("c", Some("C"), shift);
        assert!(Shortcut::new(Key::Char('c'), shift).matches(&shift_c));
        assert!(!Shortcut::new(Key::Char('c'), Modifiers::NONE).matches(&shift_c));
    }

    #[test]
    fn is_or_types_ignores_modifiers_and_case() {
        let shift = Modifiers { shift: true, ..Modifiers::NONE };
        let shift_c = KeyPress::from_gpui("c", Some("C"), shift);
        assert!(shift_c.is_or_types(&Key::Char('c')));
        let ctrl_c = KeyPress::from_gpui("c", None, CTRL);
        assert!(ctrl_c.is_or_types(&Key::Char('c')));
        let alt = Modifiers { alt: true, ..Modifiers::NONE };
        let option_c = KeyPress::from_gpui("c", Some("ç"), alt);
        assert!(option_c.is_or_types(&Key::Char('ç')));
        assert!(option_c.is_or_types(&Key::Char('c')));
        assert!(!option_c.is_or_types(&Key::Char('x')));
        let enter = KeyPress::from_gpui("enter", Some("\n"), Modifiers::NONE);
        assert!(enter.is_or_types(&Key::Enter));
        assert!(!enter.is_or_types(&Key::Char('\n')));
        let multi = KeyPress::from_gpui("e", Some("´e"), Modifiers::NONE);
        assert!(multi.is_or_types(&Key::Char('e')));
        assert!(!multi.is_or_types(&Key::Char('´')));
    }

    #[test]
    fn host_os_is_a_compile_time_constant() {
        let expected = if cfg!(target_os = "macos") { Os::MacOs } else { Os::Linux };
        assert_eq!(Os::host(), expected);
    }
}
