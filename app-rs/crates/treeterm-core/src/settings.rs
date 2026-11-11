//! The parts of `<userData>/settings.json` (shared with the Electron app) that the native
//! app reads so far. Tolerant: a missing file, a missing key or a value of the wrong type
//! falls back to the Electron default for that key, so a settings file written by either
//! app always loads. The full port (writes, migrations, every section) replaces this.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TerminalSettings {
    pub font_size: f32,
    /// CSS font-family list, e.g. `Menlo, Monaco, Consolas, monospace`.
    pub font_family: String,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub allow_osc52_clipboard: bool,
    pub max_cols: u16,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            font_family: "Menlo, Monaco, Consolas, monospace".to_owned(),
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            allow_osc52_clipboard: true,
            max_cols: 160,
        }
    }
}

impl TerminalSettings {
    /// The font families to try in order, without CSS quoting or generic families that
    /// mean nothing outside a browser.
    pub fn font_families(&self) -> Vec<String> {
        self.font_family
            .split(',')
            .map(|name| name.trim().trim_matches(['"', '\'']).to_owned())
            .filter(|name| !name.is_empty() && !matches!(name.as_str(), "monospace" | "ui-monospace"))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzerTimings {
    pub idle_debounce: Duration,
    /// Idle debounce while the workspace already carries an unread marker.
    pub idle_debounce_unread: Duration,
}

impl Default for AnalyzerTimings {
    fn default() -> Self {
        Self {
            idle_debounce: Duration::from_millis(2000),
            idle_debounce_unread: Duration::from_millis(15000),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub terminal: TerminalSettings,
    pub analyzer: AnalyzerTimings,
}

pub fn settings_path(user_data_dir: &Path) -> PathBuf {
    user_data_dir.join("settings.json")
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("{path} is not valid JSON: {source}")]
    Parse { path: PathBuf, source: serde_json::Error },
}

/// Load settings. A missing file is the defaults; an unreadable or corrupt one is an error
/// the caller must show (Electron falls back to defaults silently, which hides the problem).
pub fn load(user_data_dir: &Path) -> Result<Settings, SettingsError> {
    let path = settings_path(user_data_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(source) => return Err(SettingsError::Read { path, source }),
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|source| SettingsError::Parse { path, source })?;
    Ok(from_value(&value))
}

pub fn from_value(value: &Value) -> Settings {
    let defaults = Settings::default();
    let terminal = &value["terminal"];
    let analyzer = &value["terminalAnalyzer"];
    let d = &defaults.terminal;
    let font_size = terminal["fontSize"]
        .as_f64()
        .filter(|size| (4.0..=96.0).contains(size))
        .map_or(d.font_size, |size| size as f32);
    let cursor_style = match terminal["cursorStyle"].as_str() {
        Some("underline") => CursorStyle::Underline,
        Some("bar") => CursorStyle::Bar,
        Some("block") => CursorStyle::Block,
        _ => d.cursor_style,
    };
    let millis = |v: &Value, default: Duration| v.as_u64().map_or(default, Duration::from_millis);
    Settings {
        terminal: TerminalSettings {
            font_size,
            font_family: terminal["fontFamily"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map_or_else(|| d.font_family.clone(), str::to_owned),
            cursor_style,
            cursor_blink: terminal["cursorBlink"].as_bool().unwrap_or(d.cursor_blink),
            allow_osc52_clipboard: terminal["allowOsc52Clipboard"]
                .as_bool()
                .unwrap_or(d.allow_osc52_clipboard),
            max_cols: terminal["maxCols"]
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .filter(|n| *n >= 2)
                .unwrap_or(d.max_cols),
        },
        analyzer: AnalyzerTimings {
            idle_debounce: millis(&analyzer["idleDebounceMs"], defaults.analyzer.idle_debounce),
            idle_debounce_unread: millis(
                &analyzer["idleDebounceUnreadMs"],
                defaults.analyzer.idle_debounce_unread,
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_keys_take_electron_defaults() {
        assert_eq!(from_value(&json!({})), Settings::default());
    }

    #[test]
    fn reads_the_terminal_section() {
        let settings = from_value(&json!({
            "terminal": {
                "fontSize": 12,
                "fontFamily": "'JetBrains Mono', Menlo, monospace",
                "cursorStyle": "bar",
                "cursorBlink": false,
                "allowOsc52Clipboard": false,
                "maxCols": 200
            },
            "terminalAnalyzer": { "idleDebounceMs": 750 }
        }));
        assert_eq!(settings.terminal.font_size, 12.0);
        assert_eq!(settings.terminal.cursor_style, CursorStyle::Bar);
        assert!(!settings.terminal.cursor_blink);
        assert!(!settings.terminal.allow_osc52_clipboard);
        assert_eq!(settings.terminal.max_cols, 200);
        assert_eq!(settings.terminal.font_families(), vec!["JetBrains Mono", "Menlo"]);
        assert_eq!(settings.analyzer.idle_debounce, Duration::from_millis(750));
        assert_eq!(settings.analyzer.idle_debounce_unread, Duration::from_millis(15000));
    }

    #[test]
    fn wrong_types_fall_back_per_key() {
        let settings = from_value(&json!({
            "terminal": { "fontSize": "big", "cursorStyle": 3, "maxCols": -1, "fontFamily": "Fira Code" }
        }));
        assert_eq!(settings.terminal.font_size, 14.0);
        assert_eq!(settings.terminal.cursor_style, CursorStyle::Block);
        assert_eq!(settings.terminal.max_cols, 160);
        assert_eq!(settings.terminal.font_family, "Fira Code");
    }

    #[test]
    fn a_missing_file_is_the_defaults_and_a_corrupt_one_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap(), Settings::default());
        std::fs::write(settings_path(dir.path()), b"{not json").unwrap();
        assert!(matches!(load(dir.path()), Err(SettingsError::Parse { .. })));
    }
}
