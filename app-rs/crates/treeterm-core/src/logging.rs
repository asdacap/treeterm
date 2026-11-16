//! The app's log (stdout) and its level.
//!
//! Electron's renderer took its log level from localStorage (`treeterm:logLevel`,
//! `logger.ts`), so it could change without a rebuild and stayed set across launches. The
//! native app keeps it as `logLevel` in `ui-state.json` ([`crate::ui_state::read_log_level`]),
//! read at start-up. `RUST_LOG`, with tracing's own directives, still overrides it.

/// A level `logLevel` names (`logger.ts`'s `LEVEL_BY_NAME`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
    /// Nothing is logged.
    Silent,
}

/// The level when none is stored. Electron's renderer defaulted to warn, but its main
/// process printed everything it `console.log`ged, which is what info shows.
pub const DEFAULT_LEVEL: LogLevel = LogLevel::Info;

impl LogLevel {
    /// The level `name` names, in any case (`logger.ts` lower-cases it).
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            "silent" => Some(Self::Silent),
            _ => None,
        }
    }

    /// The level as a `tracing_subscriber::EnvFilter` directive: this level and above, from
    /// every target.
    pub fn directive(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Silent => "off",
        }
    }
}

/// What the `logLevel` stored in `ui-state.json` sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredLevel {
    /// None is stored: [`DEFAULT_LEVEL`].
    Absent,
    Known(LogLevel),
    /// It names no level: [`DEFAULT_LEVEL`], as in Electron, which ignored it silently. The
    /// app logs it.
    Unknown(String),
}

impl StoredLevel {
    /// What `stored`, the `logLevel` read, sets.
    pub fn from_stored(stored: Option<&str>) -> Self {
        match stored {
            None => Self::Absent,
            Some(name) => match LogLevel::parse(name) {
                Some(level) => Self::Known(level),
                None => Self::Unknown(name.to_owned()),
            },
        }
    }

    /// The level it sets.
    pub fn level(&self) -> LogLevel {
        match self {
            Self::Known(level) => *level,
            Self::Absent | Self::Unknown(_) => DEFAULT_LEVEL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_named_as_in_electron_in_any_case() {
        let named = [
            ("debug", LogLevel::Debug),
            ("INFO", LogLevel::Info),
            ("Warn", LogLevel::Warn),
            ("error", LogLevel::Error),
            ("Silent", LogLevel::Silent),
        ];
        for (name, level) in named {
            assert_eq!(LogLevel::parse(name), Some(level), "{name}");
        }
        for name in ["verbose", "trace", "off", "", " info"] {
            assert_eq!(LogLevel::parse(name), None, "{name:?}");
        }
    }

    #[test]
    fn each_level_is_a_directive_for_every_target() {
        let directives = [
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Warn,
            LogLevel::Error,
            LogLevel::Silent,
        ]
        .map(LogLevel::directive);
        assert_eq!(directives, ["debug", "info", "warn", "error", "off"]);
    }

    #[test]
    fn a_stored_level_applies_and_anything_else_is_the_default_info() {
        assert_eq!(DEFAULT_LEVEL, LogLevel::Info);
        let debug = StoredLevel::from_stored(Some("DEBUG"));
        assert_eq!(debug, StoredLevel::Known(LogLevel::Debug));
        assert_eq!(debug.level(), LogLevel::Debug);

        let absent = StoredLevel::from_stored(None);
        assert_eq!(absent, StoredLevel::Absent);
        assert_eq!(absent.level(), LogLevel::Info);

        let unknown = StoredLevel::from_stored(Some("verbose"));
        assert_eq!(unknown, StoredLevel::Unknown("verbose".to_owned()));
        assert_eq!(unknown.level(), LogLevel::Info);
    }
}
