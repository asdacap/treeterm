//! The application variants the dialog lists (terminal profiles, AI harnesses, custom
//! runners, web apps, browsers): their text fields and toggles, what "+ Add" creates, and
//! what makes one invalid. Free of GPUI; `instance_list` renders them.

use treeterm_core::settings::{
    AiHarnessInstance, BrowserInstance, CustomRunnerInstance, Extra, TerminalInstance, WebAppInstance,
};
use treeterm_model::apps::parse_local_url;

use super::form::SandboxSupport;

/// One kind of application variant as the dialog edits it.
pub trait InstanceForm: Clone + 'static {
    /// The text fields besides the icon.
    type Field: Copy + 'static;
    type Toggle: Copy + 'static;

    /// Electron's id prefix for new instances (`<prefix>-<Date.now()>`).
    const ID_PREFIX: &'static str;
    const INTRO: &'static str;
    const ADD_LABEL: &'static str;
    const OUTRO: &'static str;
    /// Display order.
    const FIELDS: &'static [Self::Field];
    /// Display order.
    const TOGGLES: &'static [Self::Toggle];

    /// What Electron's "+ Add" button creates.
    fn added(id: String) -> Self;
    fn id(&self) -> &str;
    fn icon(&self) -> &str;
    fn icon_mut(&mut self) -> &mut String;
    fn text(&self, field: Self::Field) -> &str;
    fn text_mut(&mut self, field: Self::Field) -> &mut String;
    fn placeholder(field: Self::Field) -> &'static str;
    fn toggle(&self, toggle: Self::Toggle) -> bool;
    fn toggle_mut(&mut self, toggle: Self::Toggle) -> &mut bool;
    fn toggle_label(toggle: Self::Toggle) -> &'static str;
    /// Whether the toggle can be changed now.
    fn toggle_enabled(&self, toggle: Self::Toggle, sandbox: SandboxSupport) -> bool;
    /// What is wrong with the instance: a warning shown under it. Save still writes it.
    fn problem(&self) -> Option<String>;
}

/// Electron's `maxLength={2}` on the icon field, in UTF-16 code units (so one emoji).
pub const MAX_ICON_UNITS: usize = 2;

/// The icon field after an edit turned `old` into `new`, leaving the caret at byte `caret`
/// (the end of what the edit inserted), and where the caret goes. As Chromium does for
/// `maxLength`, the inserted text is cut to the room the rest of the text leaves, whole
/// characters only (so a paste of "abc" keeps "ab", and an emoji with no room is dropped).
/// Deleting is always allowed, so an icon saved longer than the limit can shrink but not grow.
pub fn fit_icon(old: &str, new: &str, caret: usize) -> (String, usize) {
    if utf16_len(new) <= MAX_ICON_UNITS {
        return (new.to_owned(), caret);
    }
    // What the edit kept around the insertion; the caret tells where the insertion ends
    // when its text also ends the kept text ("ab" -> "ba|b" inserted "ba", not "b").
    let suffix = common_suffix(old, new).min(new.len() - caret);
    let prefix = common_prefix(&old[..old.len() - suffix], &new[..new.len() - suffix]);
    let (head, rest) = new.split_at(prefix);
    let (inserted, tail) = rest.split_at(rest.len() - suffix);
    let room = MAX_ICON_UNITS.saturating_sub(utf16_len(head) + utf16_len(tail));
    let mut kept = 0;
    let mut units = 0;
    for character in inserted.chars() {
        units += character.len_utf16();
        if units > room {
            break;
        }
        kept += character.len_utf8();
    }
    (format!("{head}{}{tail}", &inserted[..kept]), head.len() + kept)
}

fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// The bytes `a` and `b` start with in common, in whole characters.
fn common_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum()
}

/// The bytes `a` and `b` end with in common, in whole characters.
fn common_suffix(a: &str, b: &str) -> usize {
    a.chars().rev().zip(b.chars().rev()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum()
}

/// A name and the command it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandField {
    Name,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultToggle {
    /// Opened automatically in new workspaces.
    IsDefault,
}

// === terminal profiles ======================================================================

impl InstanceForm for TerminalInstance {
    type Field = CommandField;
    type Toggle = DefaultToggle;

    const ID_PREFIX: &'static str = "term";
    const INTRO: &'static str = "Create custom terminal profiles with startup commands. \
        These appear as separate options in the new tab menu.";
    const ADD_LABEL: &'static str = "+ Add Terminal Profile";
    const OUTRO: &'static str = "Default profiles open automatically in new workspaces.";
    const FIELDS: &'static [CommandField] = &[CommandField::Name, CommandField::Command];
    const TOGGLES: &'static [DefaultToggle] = &[DefaultToggle::IsDefault];

    fn added(id: String) -> Self {
        TerminalInstance {
            id,
            name: "New Terminal".to_owned(),
            icon: ">".to_owned(),
            startup_command: String::new(),
            is_default: false,
            extra: Extra::new(),
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn icon(&self) -> &str {
        &self.icon
    }

    fn icon_mut(&mut self) -> &mut String {
        &mut self.icon
    }

    fn text(&self, field: CommandField) -> &str {
        match field {
            CommandField::Name => &self.name,
            CommandField::Command => &self.startup_command,
        }
    }

    fn text_mut(&mut self, field: CommandField) -> &mut String {
        match field {
            CommandField::Name => &mut self.name,
            CommandField::Command => &mut self.startup_command,
        }
    }

    fn placeholder(field: CommandField) -> &'static str {
        match field {
            CommandField::Name => "Name",
            CommandField::Command => "Startup command",
        }
    }

    fn toggle(&self, toggle: DefaultToggle) -> bool {
        match toggle {
            DefaultToggle::IsDefault => self.is_default,
        }
    }

    fn toggle_mut(&mut self, toggle: DefaultToggle) -> &mut bool {
        match toggle {
            DefaultToggle::IsDefault => &mut self.is_default,
        }
    }

    fn toggle_label(toggle: DefaultToggle) -> &'static str {
        match toggle {
            DefaultToggle::IsDefault => "Default",
        }
    }

    fn toggle_enabled(&self, toggle: DefaultToggle, _sandbox: SandboxSupport) -> bool {
        match toggle {
            DefaultToggle::IsDefault => true,
        }
    }

    fn problem(&self) -> Option<String> {
        None
    }
}

// === AI harness =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessField {
    Name,
    Command,
    BackgroundColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessToggle {
    IsDefault,
    EnableSandbox,
    AllowNetwork,
    DisableScrollbar,
    KeepOnExit,
}

impl InstanceForm for AiHarnessInstance {
    type Field = HarnessField;
    type Toggle = HarnessToggle;

    const ID_PREFIX: &'static str = "ai";
    const INTRO: &'static str = "Configure AI tools like Claude, Cline, OpenCode, etc. \
        These appear as separate options in the new tab menu.";
    const ADD_LABEL: &'static str = "+ Add AI Tool";
    const OUTRO: &'static str = "Default profiles open automatically in new workspaces. \
        Sandbox settings restrict file and network access.";
    const FIELDS: &'static [HarnessField] = &[HarnessField::Name, HarnessField::Command, HarnessField::BackgroundColor];
    const TOGGLES: &'static [HarnessToggle] = &[
        HarnessToggle::IsDefault,
        HarnessToggle::EnableSandbox,
        HarnessToggle::AllowNetwork,
        HarnessToggle::DisableScrollbar,
        HarnessToggle::KeepOnExit,
    ];

    fn added(id: String) -> Self {
        AiHarnessInstance {
            id,
            name: "New AI Tool".to_owned(),
            icon: "🤖".to_owned(),
            command: String::new(),
            is_default: false,
            enable_sandbox: false,
            allow_network: true,
            background_color: "#1a1a1a".to_owned(),
            disable_scrollbar: false,
            keep_on_exit: false,
            extra: Extra::new(),
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn icon(&self) -> &str {
        &self.icon
    }

    fn icon_mut(&mut self) -> &mut String {
        &mut self.icon
    }

    fn text(&self, field: HarnessField) -> &str {
        match field {
            HarnessField::Name => &self.name,
            HarnessField::Command => &self.command,
            HarnessField::BackgroundColor => &self.background_color,
        }
    }

    fn text_mut(&mut self, field: HarnessField) -> &mut String {
        match field {
            HarnessField::Name => &mut self.name,
            HarnessField::Command => &mut self.command,
            HarnessField::BackgroundColor => &mut self.background_color,
        }
    }

    fn placeholder(field: HarnessField) -> &'static str {
        match field {
            HarnessField::Name => "Name (e.g., Claude)",
            HarnessField::Command => "Command (e.g., claude, cline, opencode)",
            HarnessField::BackgroundColor => "Background color (e.g., #1a1a24)",
        }
    }

    fn toggle(&self, toggle: HarnessToggle) -> bool {
        match toggle {
            HarnessToggle::IsDefault => self.is_default,
            HarnessToggle::EnableSandbox => self.enable_sandbox,
            HarnessToggle::AllowNetwork => self.allow_network,
            HarnessToggle::DisableScrollbar => self.disable_scrollbar,
            HarnessToggle::KeepOnExit => self.keep_on_exit,
        }
    }

    fn toggle_mut(&mut self, toggle: HarnessToggle) -> &mut bool {
        match toggle {
            HarnessToggle::IsDefault => &mut self.is_default,
            HarnessToggle::EnableSandbox => &mut self.enable_sandbox,
            HarnessToggle::AllowNetwork => &mut self.allow_network,
            HarnessToggle::DisableScrollbar => &mut self.disable_scrollbar,
            HarnessToggle::KeepOnExit => &mut self.keep_on_exit,
        }
    }

    fn toggle_label(toggle: HarnessToggle) -> &'static str {
        match toggle {
            HarnessToggle::IsDefault => "Start by Default",
            HarnessToggle::EnableSandbox => "Enable Sandbox",
            HarnessToggle::AllowNetwork => "Allow Network",
            HarnessToggle::DisableScrollbar => "Disable Scrollbar",
            HarnessToggle::KeepOnExit => "Keep Tab Open on Exit",
        }
    }

    fn toggle_enabled(&self, toggle: HarnessToggle, sandbox: SandboxSupport) -> bool {
        match toggle {
            HarnessToggle::IsDefault | HarnessToggle::DisableScrollbar | HarnessToggle::KeepOnExit => true,
            HarnessToggle::EnableSandbox => sandbox.is_available(),
            HarnessToggle::AllowNetwork => sandbox.is_available() && self.enable_sandbox,
        }
    }

    fn problem(&self) -> Option<String> {
        None
    }
}

// === custom runners =========================================================================

impl InstanceForm for CustomRunnerInstance {
    type Field = CommandField;
    type Toggle = DefaultToggle;

    const ID_PREFIX: &'static str = "runner";
    const INTRO: &'static str = "Configure custom runners with template commands. \
        Use {{workspace_path}} in the command to insert the workspace path at runtime. \
        These appear as separate options in the new tab menu.";
    const ADD_LABEL: &'static str = "+ Add Custom Runner";
    const OUTRO: &'static str = "Default runners open automatically in new workspaces.";
    const FIELDS: &'static [CommandField] = &[CommandField::Name, CommandField::Command];
    const TOGGLES: &'static [DefaultToggle] = &[DefaultToggle::IsDefault];

    fn added(id: String) -> Self {
        CustomRunnerInstance {
            id,
            name: "New Runner".to_owned(),
            icon: "▶".to_owned(),
            command_template: String::new(),
            is_default: false,
            extra: Extra::new(),
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn icon(&self) -> &str {
        &self.icon
    }

    fn icon_mut(&mut self) -> &mut String {
        &mut self.icon
    }

    fn text(&self, field: CommandField) -> &str {
        match field {
            CommandField::Name => &self.name,
            CommandField::Command => &self.command_template,
        }
    }

    fn text_mut(&mut self, field: CommandField) -> &mut String {
        match field {
            CommandField::Name => &mut self.name,
            CommandField::Command => &mut self.command_template,
        }
    }

    fn placeholder(field: CommandField) -> &'static str {
        match field {
            CommandField::Name => "Name (e.g., Rider)",
            CommandField::Command => "Command template (e.g., rider {{workspace_path}})",
        }
    }

    fn toggle(&self, toggle: DefaultToggle) -> bool {
        match toggle {
            DefaultToggle::IsDefault => self.is_default,
        }
    }

    fn toggle_mut(&mut self, toggle: DefaultToggle) -> &mut bool {
        match toggle {
            DefaultToggle::IsDefault => &mut self.is_default,
        }
    }

    fn toggle_label(toggle: DefaultToggle) -> &'static str {
        match toggle {
            DefaultToggle::IsDefault => "Default",
        }
    }

    fn toggle_enabled(&self, toggle: DefaultToggle, _sandbox: SandboxSupport) -> bool {
        match toggle {
            DefaultToggle::IsDefault => true,
        }
    }

    fn problem(&self) -> Option<String> {
        None
    }
}

// === web apps ===============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebAppToggle {
    KeepOnExit,
    IsDefault,
}

impl InstanceForm for WebAppInstance {
    type Field = CommandField;
    type Toggle = WebAppToggle;

    const ID_PREFIX: &'static str = "webapp";
    const INTRO: &'static str = "Configure web apps such as dev servers. Use $PORT in the command; \
        it is replaced with a free port, and the Browser tab opens that port once the server responds. \
        On remote sessions the port is forwarded over SSH first.";
    const ADD_LABEL: &'static str = "+ Add Web App";
    const OUTRO: &'static str = "Default web apps open automatically in new workspaces.";
    const FIELDS: &'static [CommandField] = &[CommandField::Name, CommandField::Command];
    const TOGGLES: &'static [WebAppToggle] = &[WebAppToggle::KeepOnExit, WebAppToggle::IsDefault];

    fn added(id: String) -> Self {
        WebAppInstance {
            id,
            name: "New Web App".to_owned(),
            icon: "🌐".to_owned(),
            command: "npm run dev -- -p $PORT".to_owned(),
            is_default: false,
            keep_on_exit: true,
            extra: Extra::new(),
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn icon(&self) -> &str {
        &self.icon
    }

    fn icon_mut(&mut self) -> &mut String {
        &mut self.icon
    }

    fn text(&self, field: CommandField) -> &str {
        match field {
            CommandField::Name => &self.name,
            CommandField::Command => &self.command,
        }
    }

    fn text_mut(&mut self, field: CommandField) -> &mut String {
        match field {
            CommandField::Name => &mut self.name,
            CommandField::Command => &mut self.command,
        }
    }

    fn placeholder(field: CommandField) -> &'static str {
        match field {
            CommandField::Name => "Name (e.g., Next.js)",
            CommandField::Command => "Command (e.g., npm run dev -- -p $PORT)",
        }
    }

    fn toggle(&self, toggle: WebAppToggle) -> bool {
        match toggle {
            WebAppToggle::KeepOnExit => self.keep_on_exit,
            WebAppToggle::IsDefault => self.is_default,
        }
    }

    fn toggle_mut(&mut self, toggle: WebAppToggle) -> &mut bool {
        match toggle {
            WebAppToggle::KeepOnExit => &mut self.keep_on_exit,
            WebAppToggle::IsDefault => &mut self.is_default,
        }
    }

    fn toggle_label(toggle: WebAppToggle) -> &'static str {
        match toggle {
            WebAppToggle::KeepOnExit => "Keep Tab Open on Exit",
            WebAppToggle::IsDefault => "Default",
        }
    }

    fn toggle_enabled(&self, toggle: WebAppToggle, _sandbox: SandboxSupport) -> bool {
        match toggle {
            WebAppToggle::KeepOnExit | WebAppToggle::IsDefault => true,
        }
    }

    fn problem(&self) -> Option<String> {
        None
    }
}

// === browsers ===============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserField {
    Name,
    Url,
}

impl InstanceForm for BrowserInstance {
    type Field = BrowserField;
    type Toggle = DefaultToggle;

    const ID_PREFIX: &'static str = "browser";
    const INTRO: &'static str = "Open a page that is already being served, such as a dashboard. \
        The URL must be on localhost; on remote sessions its port is forwarded over SSH first.";
    const ADD_LABEL: &'static str = "+ Add Browser";
    const OUTRO: &'static str = "Default browsers open automatically in new workspaces.";
    const FIELDS: &'static [BrowserField] = &[BrowserField::Name, BrowserField::Url];
    const TOGGLES: &'static [DefaultToggle] = &[DefaultToggle::IsDefault];

    fn added(id: String) -> Self {
        BrowserInstance {
            id,
            name: "New Browser".to_owned(),
            icon: "🧭".to_owned(),
            url: "http://localhost:8080/".to_owned(),
            is_default: false,
            extra: Extra::new(),
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn icon(&self) -> &str {
        &self.icon
    }

    fn icon_mut(&mut self) -> &mut String {
        &mut self.icon
    }

    fn text(&self, field: BrowserField) -> &str {
        match field {
            BrowserField::Name => &self.name,
            BrowserField::Url => &self.url,
        }
    }

    fn text_mut(&mut self, field: BrowserField) -> &mut String {
        match field {
            BrowserField::Name => &mut self.name,
            BrowserField::Url => &mut self.url,
        }
    }

    fn placeholder(field: BrowserField) -> &'static str {
        match field {
            BrowserField::Name => "Name (e.g., Grafana)",
            BrowserField::Url => "URL (e.g., http://localhost:8080/)",
        }
    }

    fn toggle(&self, toggle: DefaultToggle) -> bool {
        match toggle {
            DefaultToggle::IsDefault => self.is_default,
        }
    }

    fn toggle_mut(&mut self, toggle: DefaultToggle) -> &mut bool {
        match toggle {
            DefaultToggle::IsDefault => &mut self.is_default,
        }
    }

    fn toggle_label(toggle: DefaultToggle) -> &'static str {
        match toggle {
            DefaultToggle::IsDefault => "Default",
        }
    }

    fn toggle_enabled(&self, toggle: DefaultToggle, _sandbox: SandboxSupport) -> bool {
        match toggle {
            DefaultToggle::IsDefault => true,
        }
    }

    /// `parseLocalUrl`'s error.
    fn problem(&self) -> Option<String> {
        parse_local_url(&self.url).err()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every text field reads and writes its own value.
    fn fields_round_trip<T: InstanceForm>() {
        let mut instance = T::added("x-1".into());
        assert_eq!(instance.id(), "x-1");
        for (index, field) in T::FIELDS.iter().enumerate() {
            *instance.text_mut(*field) = format!("value {index}");
            assert!(!T::placeholder(*field).is_empty());
        }
        for (index, field) in T::FIELDS.iter().enumerate() {
            assert_eq!(instance.text(*field), format!("value {index}"));
        }
        *instance.icon_mut() = "I".into();
        assert_eq!(instance.icon(), "I");
        for toggle in T::TOGGLES {
            let before = instance.toggle(*toggle);
            *instance.toggle_mut(*toggle) = !before;
            assert_eq!(instance.toggle(*toggle), !before, "{}", T::toggle_label(*toggle));
        }
    }

    #[test]
    fn every_kind_maps_its_fields_and_toggles() {
        fields_round_trip::<TerminalInstance>();
        fields_round_trip::<AiHarnessInstance>();
        fields_round_trip::<CustomRunnerInstance>();
        fields_round_trip::<WebAppInstance>();
        fields_round_trip::<BrowserInstance>();
    }

    #[test]
    fn added_instances_match_electron() {
        let harness = AiHarnessInstance::added("ai-1".into());
        assert_eq!((harness.name.as_str(), harness.icon.as_str()), ("New AI Tool", "🤖"));
        assert!(harness.allow_network && !harness.enable_sandbox && !harness.keep_on_exit);
        assert_eq!(harness.background_color, "#1a1a1a");
        let web_app = WebAppInstance::added("webapp-1".into());
        assert_eq!(web_app.command, "npm run dev -- -p $PORT");
        assert!(web_app.keep_on_exit);
        assert_eq!(TerminalInstance::added("term-1".into()).icon, ">");
        assert_eq!(CustomRunnerInstance::added("runner-1".into()).name, "New Runner");
        assert_eq!(BrowserInstance::added("browser-1".into()).problem(), None);
    }

    #[test]
    fn sandbox_toggles_need_a_sandbox_and_network_needs_it_enabled() {
        let mut harness = AiHarnessInstance::added("ai-1".into());
        let missing = SandboxSupport::MissingBwrap;
        let available = SandboxSupport::Available;
        assert!(!harness.toggle_enabled(HarnessToggle::EnableSandbox, missing));
        assert!(harness.toggle_enabled(HarnessToggle::EnableSandbox, available));
        assert!(!harness.toggle_enabled(HarnessToggle::AllowNetwork, available));
        harness.enable_sandbox = true;
        assert!(harness.toggle_enabled(HarnessToggle::AllowNetwork, available));
        assert!(!harness.toggle_enabled(HarnessToggle::AllowNetwork, missing));
        assert!(harness.toggle_enabled(HarnessToggle::KeepOnExit, missing));
        assert!(TerminalInstance::added("t".into()).toggle_enabled(DefaultToggle::IsDefault, missing));
        assert!(WebAppInstance::added("w".into()).toggle_enabled(WebAppToggle::KeepOnExit, missing));
    }

    #[test]
    fn browser_urls_must_be_local() {
        let mut browser = BrowserInstance::added("browser-1".into());
        // As WHATWG URL reads them (`parseLocalUrl`).
        for url in [
            "http://localhost:3000/grafana",
            "https://127.0.0.1/",
            "HTTP://localhost:3000",
            " http://localhost:3000 ",
            "http://127.1:3000/",
            "http:localhost:8080",
        ] {
            browser.url = url.into();
            assert_eq!(browser.problem(), None, "{url}");
        }
        for url in [
            "http://example.com/",
            "file:///etc/passwd",
            "localhost:8080",
            "",
            "http://localhost:99999/",
            "http://localhost:abc/",
            "http://evil.com\\@localhost/",
        ] {
            browser.url = url.into();
            assert_eq!(browser.problem(), Some(format!("URL must be http(s)://localhost or 127.0.0.1: {url}")));
        }
        for kind_without_problems in [
            TerminalInstance::added("t".into()).problem(),
            AiHarnessInstance::added("a".into()).problem(),
            CustomRunnerInstance::added("r".into()).problem(),
            WebAppInstance::added("w".into()).problem(),
        ] {
            assert_eq!(kind_without_problems, None);
        }
    }

    /// `fit_icon` for an edit whose insertion ends at the `|` in `new`.
    fn fit(old: &str, new: &str) -> String {
        let caret = new.find('|').expect("the test marks the caret");
        let (text, caret) = fit_icon(old, &new.replacen('|', "", 1), caret);
        format!("{}|{}", &text[..caret], &text[caret..])
    }

    #[test]
    fn edits_within_two_utf16_units_stand() {
        assert_eq!(fit("", ">|"), ">|");
        assert_eq!(fit("", "🤖|"), "🤖|");
        assert_eq!(fit("a", "ab|"), "ab|");
        assert_eq!(fit("a", "b|a"), "b|a");
        assert_eq!(fit("ab", "x|"), "x|");
        assert_eq!(fit("ab", "|b"), "|b");
        assert_eq!(fit("🤖", "|"), "|");
    }

    /// Chromium's `maxLength` cuts what is pasted or typed to the room left, in whole
    /// characters, where the Kit's `validate` rejected the whole edit.
    #[test]
    fn insertions_are_cut_to_the_room_left() {
        assert_eq!(fit("", "abc|"), "ab|");
        assert_eq!(fit("x", "xabc|"), "xa|");
        assert_eq!(fit("ab", "abc|"), "ab|");
        // Pasted before the text.
        assert_eq!(fit("a", "bc|a"), "b|a");
        // Pasted over everything, or over part of it.
        assert_eq!(fit("ab", "xyz|"), "xy|");
        assert_eq!(fit("ab", "xyz|b"), "x|b");
        // The pasted text repeats what follows it: the caret tells where the paste ends.
        assert_eq!(fit("ab", "ba|b"), "b|b");
        // An emoji is two units and is never split.
        assert_eq!(fit("", "🤖🤖|"), "🤖|");
        assert_eq!(fit("a", "a🤖|"), "a|");
        assert_eq!(fit("a", "a🤖b|"), "a|");
        assert_eq!(fit("", "a🤖|"), "a|");
    }

    /// An icon saved longer than the field allows can shrink but not grow.
    #[test]
    fn a_long_saved_icon_only_shrinks() {
        assert_eq!(fit("abc", "abcd|"), "abc|");
        assert_eq!(fit("abc", "ab|"), "ab|");
        assert_eq!(fit("abcd", "ab|d"), "ab|d");
        assert_eq!(fit("abcd", "ax|d"), "a|d");
    }
}
