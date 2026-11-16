//! Electron's locale, and dates, times, numbers and string order in it.
//!
//! Electron's renderer formats dates and numbers (`toLocaleString`, …) and compares strings
//! (`localeCompare`) with ICU in its *application* locale: not the system's preferred
//! language as is, but the closest of the UI languages Electron ships. On macOS that is the
//! localization `NSBundle` picks for Electron.app from the preferred languages (English
//! (Malaysia) then English picks `en`, so en-US dates; English (Australia) alone picks
//! `en_GB`); elsewhere it is Chromium's `CheckAndResolveLocale` over `LANGUAGE`, `LC_ALL`,
//! `LC_MESSAGES` and `LANG`.
//!
//! The formats are what Electron 33 prints in each of those locales, read off its renderer
//! (ICU 74, CLDR 44, through V8, which prints CLDR's narrow no-break space before AM/PM as a
//! plain space). Order comes from ICU4X's collator, which sorts as Chromium's does.

mod electron_locales;

use std::cmp::Ordering;
use std::sync::OnceLock;

use icu_calendar::types::EraYear;
use icu_calendar::{Date, cal};
use icu_collator::options::CollatorOptions;
use icu_collator::{Collator, CollatorBorrowed};
use icu_locale_core::Locale;

use crate::clock::LocalTime;
use electron_locales::ELECTRON_LOCALES;

/// One of the UI languages Electron ships, with the date and number formats its renderer
/// prints there.
#[derive(Debug, PartialEq, Eq)]
pub struct ElectronLocale {
    /// The application locale (`app.getLocale()`), as Chromium spells it: `locales/<app>.pak`,
    /// `Electron.app/Contents/Resources/<app>.lproj` with `_` for `-` and `en` for `en-US`.
    app: &'static str,
    /// The locale ICU runs in (`base::i18n::ICULocaleName`): Spanish gets a region, `es-ES`
    /// for `es` and `es-MX` for Latin American `es-419`.
    icu: &'static str,
    calendar: Calendar,
    /// The locale's zero; its other digits follow it.
    zero: char,
    grouping: Grouping,
    day_periods: DayPeriods,
    /// `toLocaleString()`: numeric date with the full year, and the time with seconds.
    date_time: &'static str,
    /// `toLocaleDateString()`.
    date: &'static str,
    /// `toLocaleTimeString()`.
    time: &'static str,
    /// `toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })`.
    time_two_digit: &'static str,
}

/// The calendar a locale counts years, months and days in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Calendar {
    Gregorian,
    /// Thai: the Gregorian months, years from 543 BC.
    Buddhist,
    /// Farsi: the solar Hijri calendar.
    Persian,
}

/// Where a locale puts a separator between the digits of a whole number, and which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grouping {
    /// Between groups of three: `1,000`, `1,234,567`.
    Threes(&'static str),
    /// Between groups of three, from five digits on: `1000`, `10 000`.
    ThreesFromTenThousand(&'static str),
    /// Before the last three digits, then between pairs: `1,000`, `1,00,000`, `12,34,567`.
    Indian(&'static str),
}

/// How a locale's clock names the halves of the day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DayPeriods {
    /// Its times are 0–23 and never say AM or PM.
    TwentyFourHour,
    /// Its times are 1–12 followed (or preceded) by these.
    AmPm(&'static str, &'static str),
}

// Templates are literal text with fields in braces: `{y}` the year, `{M}`/`{MM}` the month
// (`MM` padded to two digits), `{d}`/`{dd}` the day, `{H}`/`{HH}` the hour 0–23, `{h}`/`{hh}`
// the hour 1–12, `{mm}` minutes, `{ss}` seconds, `{a}` AM or PM.
impl ElectronLocale {
    /// The locale Electron's renderer runs in on this machine.
    pub fn current() -> &'static Self {
        static CURRENT: OnceLock<&'static ElectronLocale> = OnceLock::new();
        CURRENT.get_or_init(app_locale)
    }

    /// `new Date(…).toLocaleString()`; `None` when `t` is not a calendar date.
    pub fn date_time(&self, t: LocalTime) -> Option<String> {
        Some(self.render(self.date_time, t, self.calendar_date(t)?))
    }

    /// `new Date(…).toLocaleDateString()`; `None` when `t` is not a calendar date.
    pub fn date(&self, t: LocalTime) -> Option<String> {
        Some(self.render(self.date, t, self.calendar_date(t)?))
    }

    /// `new Date(…).toLocaleTimeString()`.
    pub fn time(&self, t: LocalTime) -> String {
        self.render(self.time, t, (t.year, t.month, t.day))
    }

    /// `toLocaleTimeString` with a 2-digit hour, minute and second.
    pub fn time_two_digit(&self, t: LocalTime) -> String {
        self.render(self.time_two_digit, t, (t.year, t.month, t.day))
    }

    /// `n.toLocaleString()`: in this locale's digits, grouped as it groups them (`25.003` in
    /// German, `25 003` in French, `۲۵٬۰۰۳` in Farsi, `12,34,567` in Hindi).
    pub fn number(&self, n: u64) -> String {
        let digits = n.to_string();
        let (separator, fewest_grouped) = match self.grouping {
            Grouping::Threes(separator) | Grouping::Indian(separator) => (separator, 4),
            Grouping::ThreesFromTenThousand(separator) => (separator, 5),
        };
        // Whether a group starts `from_end` digits before the end.
        let starts_group = |from_end: usize| match self.grouping {
            Grouping::Threes(_) | Grouping::ThreesFromTenThousand(_) => from_end.is_multiple_of(3),
            Grouping::Indian(_) => from_end == 3 || (from_end > 3 && (from_end - 3).is_multiple_of(2)),
        };
        let mut out = String::new();
        for (index, c) in digits.chars().enumerate() {
            if index > 0 && digits.len() >= fewest_grouped && starts_group(digits.len() - index) {
                out.push_str(separator);
            }
            out.push(self.digit(c));
        }
        out
    }

    /// Year, month and day of `t` in this locale's calendar.
    fn calendar_date(&self, t: LocalTime) -> Option<(i32, u32, u32)> {
        fn convert<C: icu_calendar::Calendar<Year = EraYear>>(t: LocalTime, calendar: C) -> Option<(i32, u32, u32)> {
            let iso = Date::try_new_iso(t.year, u8::try_from(t.month).ok()?, u8::try_from(t.day).ok()?).ok()?;
            let date = iso.to_calendar(calendar);
            Some((date.era_year().year, u32::from(date.month().ordinal), u32::from(date.day_of_month().0)))
        }
        match self.calendar {
            Calendar::Gregorian => Some((t.year, t.month, t.day)),
            Calendar::Buddhist => convert(t, cal::Buddhist),
            Calendar::Persian => convert(t, cal::Persian),
        }
    }

    fn render(&self, template: &str, t: LocalTime, (year, month, day): (i32, u32, u32)) -> String {
        let hour12 = match t.hour % 12 {
            0 => 12,
            hour => hour,
        };
        let mut out = String::new();
        let mut rest = template;
        while let Some((literal, after)) = rest.split_once('{') {
            out.push_str(literal);
            let (field, after) = after.split_once('}').expect("template fields are closed");
            match field {
                "y" => self.push_number(&mut out, i64::from(year), 1),
                "M" => self.push_number(&mut out, i64::from(month), 1),
                "MM" => self.push_number(&mut out, i64::from(month), 2),
                "d" => self.push_number(&mut out, i64::from(day), 1),
                "dd" => self.push_number(&mut out, i64::from(day), 2),
                "H" => self.push_number(&mut out, i64::from(t.hour), 1),
                "HH" => self.push_number(&mut out, i64::from(t.hour), 2),
                "h" => self.push_number(&mut out, i64::from(hour12), 1),
                "hh" => self.push_number(&mut out, i64::from(hour12), 2),
                "mm" => self.push_number(&mut out, i64::from(t.minute), 2),
                "ss" => self.push_number(&mut out, i64::from(t.second), 2),
                "a" => match self.day_periods {
                    DayPeriods::AmPm(am, pm) => out.push_str(if t.hour < 12 { am } else { pm }),
                    DayPeriods::TwentyFourHour => unreachable!("{}'s 24-hour templates have no AM/PM", self.app),
                },
                other => unreachable!("{} has an unknown template field {{{other}}}", self.app),
            }
            rest = after;
        }
        out.push_str(rest);
        out
    }

    /// `value` padded with zeros to `width` digits, in this locale's digits.
    fn push_number(&self, out: &mut String, value: i64, width: usize) {
        out.extend(format!("{value:0width$}").chars().map(|c| self.digit(c)));
    }

    /// ASCII digit `c` as this locale writes it; any other character as it is.
    fn digit(&self, c: char) -> char {
        match c.to_digit(10) {
            Some(digit) => char::from_u32(u32::from(self.zero) + digit).expect("a locale's ten digits are consecutive"),
            None => c,
        }
    }
}

/// `a.localeCompare(b)` in Electron's locale.
pub fn compare(a: &str, b: &str) -> Ordering {
    static COLLATOR: OnceLock<CollatorBorrowed<'static>> = OnceLock::new();
    COLLATOR.get_or_init(|| collator(ElectronLocale::current())).compare(a, b)
}

/// V8's `localeCompare` collator: the locale's, with ICU's default options (tertiary
/// strength, punctuation not ignored).
fn collator(locale: &ElectronLocale) -> CollatorBorrowed<'static> {
    let icu: Locale = locale.icu.parse().expect("Electron's locales are BCP-47 tags");
    Collator::try_new((&icu).into(), CollatorOptions::default())
        .expect("compiled collation data covers every locale (falling back to the root)")
}

/// The locale Chromium falls back to, and the one it calls the `en` localization.
fn fallback() -> &'static ElectronLocale {
    shipped("en-US").expect("Electron ships en-US")
}

/// The shipped locale named `app` (any case).
fn shipped(app: &str) -> Option<&'static ElectronLocale> {
    ELECTRON_LOCALES.iter().find(|locale| locale.app.eq_ignore_ascii_case(app))
}

#[cfg(target_os = "macos")]
fn app_locale() -> &'static ElectronLocale {
    bundle_localization(Preferences::User)
}

#[cfg(not(target_os = "macos"))]
fn app_locale() -> &'static ElectronLocale {
    resolve_from_env(&|name| std::env::var_os(name).and_then(|value| value.to_str().map(str::to_owned)))
}

/// Whose preferred languages to match against Electron's localizations.
#[cfg(target_os = "macos")]
enum Preferences {
    /// The user's (`AppleLanguages`), as `NSBundle` reads them.
    User,
    #[cfg(test)]
    Given(&'static [&'static str]),
}

/// The application locale Electron's macOS start-up picks
/// (`l10n_util::OverrideLocaleWithCocoaLocale`): the first of Electron.app's
/// `preferredLocalizations`, with `_` read as `-` and `en` as `en-US`.
#[cfg(target_os = "macos")]
fn bundle_localization(preferences: Preferences) -> &'static ElectronLocale {
    use core_foundation::array::{CFArray, CFArrayRef};
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_foundation_sys::bundle::CFBundleCopyLocalizationsForPreferences;

    let lproj = |locale: &ElectronLocale| -> String {
        match locale.app {
            "en-US" => "en".to_owned(),
            app => app.replace('-', "_"),
        }
    };
    let available: Vec<CFString> = ELECTRON_LOCALES.iter().map(|locale| CFString::new(&lproj(locale))).collect();
    let available = CFArray::from_CFTypes(&available);
    // CoreFoundation reads a NULL preference array as the user's.
    let preferred: Option<CFArray<CFString>> = match preferences {
        Preferences::User => None,
        #[cfg(test)]
        Preferences::Given(languages) => {
            let languages: Vec<CFString> = languages.iter().map(|language| CFString::new(language)).collect();
            Some(CFArray::from_CFTypes(&languages))
        }
    };
    let preferred_ref: CFArrayRef = preferred.as_ref().map_or(std::ptr::null(), |array| array.as_concrete_TypeRef());
    // SAFETY: both arrays hold CFStrings and outlive the call. The result follows the Copy
    // rule, so it is ours to release.
    let matched = unsafe { CFBundleCopyLocalizationsForPreferences(available.as_concrete_TypeRef(), preferred_ref) };
    if matched.is_null() {
        return fallback();
    }
    // SAFETY: a non-NULL array of the CFStrings passed in, owned by us (see above).
    let matched: CFArray<CFString> = unsafe { CFArray::wrap_under_create_rule(matched) };
    let Some(first) = matched.get(0) else {
        return fallback();
    };
    let first = first.to_string();
    ELECTRON_LOCALES.iter().find(|locale| lproj(locale) == first).unwrap_or_else(fallback)
}

/// The application locale Chromium picks off macOS (`l10n_util::GetApplicationLocale`): the
/// first of the user's languages (glib's precedence: `LANGUAGE`, a `:` list, then `LC_ALL`,
/// `LC_MESSAGES`, `LANG`) that `CheckAndResolveLocale` maps to a shipped locale; else en-US.
#[cfg(any(test, not(target_os = "macos")))]
fn resolve_from_env(var: &dyn Fn(&str) -> Option<String>) -> &'static ElectronLocale {
    let languages = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(var)
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    languages
        .split(':')
        .filter_map(chromium_candidate)
        .find_map(|candidate| check_and_resolve(&candidate))
        .unwrap_or_else(fallback)
}

/// A POSIX locale name (`de_AT.UTF-8`, `ca_ES@valencia`) as Chromium's candidate (`de-AT`);
/// `None` for the C locale and empty entries, which name no language.
#[cfg(any(test, not(target_os = "macos")))]
fn chromium_candidate(posix: &str) -> Option<String> {
    match posix.split(['.', '@']).next() {
        None | Some("" | "C" | "POSIX") => None,
        Some(name) => Some(name.replace('_', "-")),
    }
}

/// `l10n_util::CheckAndResolveLocale`: `locale` if Electron ships it, else the shipped locale
/// Chromium maps its language and region to.
#[cfg(any(test, not(target_os = "macos")))]
fn check_and_resolve(locale: &str) -> Option<&'static ElectronLocale> {
    if let Some(found) = shipped(locale) {
        return Some(found);
    }
    let lowercase = locale.to_ascii_lowercase();
    let (language, region) = match lowercase.split_once('-') {
        Some((language, region)) => (language, Some(region)),
        None => (lowercase.as_str(), None),
    };
    if let Some(region) = region {
        let regional = match (language, region) {
            ("es", "es") => "es",
            ("es", _) => "es-419",
            ("pt", _) => "pt-PT",
            ("zh", "hk" | "mo") => "zh-TW",
            ("zh", _) => "zh-CN",
            ("en", "au" | "ca" | "in" | "nz" | "za") => "en-GB",
            ("en", _) => "en-US",
            (language, _) => language,
        };
        if let Some(found) = shipped(regional) {
            return Some(found);
        }
    }
    let alias = match language {
        "en" => "en-US",
        "iw" => "he",
        "no" => "nb",
        "pt" => "pt-BR",
        "tl" => "fil",
        "zh" => "zh-CN",
        _ => return None,
    };
    shipped(alias)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locale(app: &str) -> &'static ElectronLocale {
        shipped(app).unwrap()
    }

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> LocalTime {
        LocalTime { year, month, day, hour, minute, second }
    }

    fn sorted<'a>(app: &str, names: &[&'a str]) -> Vec<&'a str> {
        let collator = collator(locale(app));
        let mut names = names.to_vec();
        names.sort_by(|a, b| collator.compare(a, b));
        names
    }

    // Expected strings below are Electron 33's, e.g.
    // `new Date(2026, 10, 16, 17, 57, 12).toLocaleString('en-GB')`.

    #[test]
    fn date_times_follow_the_locale_as_to_locale_string_does() {
        let evening = at(2026, 11, 16, 17, 57, 12);
        let morning = at(2026, 1, 5, 9, 5, 7);
        let cases = [
            ("en-US", "11/16/2026, 5:57:12 PM", "1/5/2026, 9:05:07 AM"),
            ("en-GB", "16/11/2026, 17:57:12", "05/01/2026, 09:05:07"),
            ("de", "16.11.2026, 17:57:12", "5.1.2026, 09:05:07"),
            ("fr", "16/11/2026 17:57:12", "05/01/2026 09:05:07"),
            ("ja", "2026/11/16 17:57:12", "2026/1/5 9:05:07"),
            ("ko", "2026. 11. 16. 오후 5:57:12", "2026. 1. 5. 오전 9:05:07"),
            ("zh-TW", "2026/11/16 下午5:57:12", "2026/1/5 上午9:05:07"),
            ("fi", "16.11.2026 klo 17.57.12", "5.1.2026 klo 9.05.07"),
            ("vi", "17:57:12 16/11/2026", "09:05:07 5/1/2026"),
            ("ar", "16\u{200f}/11\u{200f}/2026 5:57:12 م", "5\u{200f}/1\u{200f}/2026 9:05:07 ص"),
        ];
        for (app, evening_text, morning_text) in cases {
            assert_eq!(locale(app).date_time(evening).as_deref(), Some(evening_text), "{app}");
            assert_eq!(locale(app).date_time(morning).as_deref(), Some(morning_text), "{app}");
        }
        // Midnight and noon on a 12-hour clock.
        assert_eq!(locale("en-US").date_time(at(2026, 1, 5, 0, 0, 5)).as_deref(), Some("1/5/2026, 12:00:05 AM"));
        assert_eq!(locale("en-US").date_time(at(2026, 7, 4, 12, 0, 0)).as_deref(), Some("7/4/2026, 12:00:00 PM"));
    }

    #[test]
    fn spanish_runs_in_spain_and_latin_american_spanish_in_mexico() {
        let evening = at(2026, 11, 16, 17, 57, 12);
        assert_eq!(locale("es").icu, "es-ES");
        assert_eq!(locale("es").date_time(evening).as_deref(), Some("16/11/2026, 17:57:12"));
        assert_eq!(locale("es-419").icu, "es-MX");
        assert_eq!(locale("es-419").date_time(evening).as_deref(), Some("16/11/2026, 5:57:12 p.m."));
    }

    #[test]
    fn dates_follow_the_locale_as_to_locale_date_string_does() {
        let t = at(2026, 1, 5, 9, 5, 7);
        let cases = [
            ("en-US", "1/5/2026"),
            ("en-GB", "05/01/2026"),
            ("de", "5.1.2026"),
            ("nl", "5-1-2026"),
            ("hu", "2026. 01. 05."),
            ("ko", "2026. 1. 5."),
            ("sv", "2026-01-05"),
            ("pl", "5.01.2026"),
        ];
        for (app, text) in cases {
            assert_eq!(locale(app).date(t).as_deref(), Some(text), "{app}");
        }
    }

    #[test]
    fn thai_and_farsi_dates_use_their_calendars_and_digits() {
        let evening = at(2026, 11, 16, 17, 57, 12);
        assert_eq!(locale("th").date(evening).as_deref(), Some("16/11/2569"));
        assert_eq!(locale("fa").date(evening).as_deref(), Some("۱۴۰۵/۸/۲۵"));
        assert_eq!(locale("fa").date_time(evening).as_deref(), Some("۱۴۰۵/۸/۲۵, ۱۷:۵۷:۱۲"));
        assert_eq!(locale("bn").date(evening).as_deref(), Some("১৬/১১/২০২৬"));
        // Not a calendar date.
        assert_eq!(locale("fa").date(at(2026, 2, 30, 0, 0, 0)), None);
        assert_eq!(locale("th").date_time(at(2026, 13, 1, 0, 0, 0)), None);
    }

    #[test]
    fn times_follow_the_locale_as_to_locale_time_string_does() {
        let evening = at(2026, 11, 16, 17, 57, 12);
        let morning = at(2026, 1, 5, 9, 5, 7);
        let cases = [
            ("en-US", "5:57:12 PM", "9:05:07 AM"),
            ("en-GB", "17:57:12", "09:05:07"),
            ("de", "17:57:12", "09:05:07"),
            ("ja", "17:57:12", "9:05:07"),
            ("fi", "17.57.12", "9.05.07"),
            ("bg", "17:57:12 ч.", "09:05:07 ч."),
            ("ko", "오후 5:57:12", "오전 9:05:07"),
            ("zh-TW", "下午5:57:12", "上午9:05:07"),
            ("ta", "PM 5:57:12", "AM 9:05:07"),
            ("es-419", "5:57:12 p.m.", "9:05:07 a.m."),
            ("fa", "۱۷:۵۷:۱۲", "۹:۰۵:۰۷"),
        ];
        for (app, evening_text, morning_text) in cases {
            assert_eq!(locale(app).time(evening), evening_text, "{app}");
            assert_eq!(locale(app).time(morning), morning_text, "{app}");
        }
        // Midnight and noon on a 12-hour clock; midnight on a 24-hour one.
        assert_eq!(locale("en-US").time(at(2026, 1, 5, 0, 0, 5)), "12:00:05 AM");
        assert_eq!(locale("en-US").time(at(2026, 3, 25, 12, 0, 0)), "12:00:00 PM");
        assert_eq!(locale("ja").time(at(2026, 1, 5, 0, 0, 5)), "0:00:05");
        assert_eq!(locale("en-GB").time(at(2026, 1, 5, 0, 0, 5)), "00:00:05");
    }

    #[test]
    fn two_digit_times_follow_the_locale_hour_cycle() {
        let afternoon = at(2026, 1, 5, 14, 15, 30);
        let cases = [
            ("en-US", "02:15:30 PM"),
            ("en-GB", "14:15:30"),
            ("de", "14:15:30"),
            ("ja", "14:15:30"),
            ("zh-CN", "14:15:30"),
            ("ko", "오후 02:15:30"),
            ("zh-TW", "下午02:15:30"),
            ("ta", "PM 02:15:30"),
            ("da", "14.15.30"),
        ];
        for (app, text) in cases {
            assert_eq!(locale(app).time_two_digit(afternoon), text, "{app}");
        }
        let en_us = locale("en-US");
        assert_eq!(en_us.time_two_digit(at(2026, 1, 5, 0, 0, 5)), "12:00:05 AM");
        assert_eq!(en_us.time_two_digit(at(2026, 1, 5, 12, 0, 0)), "12:00:00 PM");
        assert_eq!(en_us.time_two_digit(at(2026, 1, 5, 9, 5, 7)), "09:05:07 AM");
        assert_eq!(locale("en-GB").time_two_digit(at(2026, 1, 5, 0, 0, 5)), "00:00:05");
    }

    #[test]
    fn numbers_follow_the_locale_as_to_locale_string_does() {
        // `n.toLocaleString(<icu>)` in Electron 33 for 999, 1000, 25003, 100000 and 1234567.
        let cases = [
            ("en-US", ["999", "1,000", "25,003", "100,000", "1,234,567"]),
            ("de", ["999", "1.000", "25.003", "100.000", "1.234.567"]),
            ("fr", ["999", "1\u{202f}000", "25\u{202f}003", "100\u{202f}000", "1\u{202f}234\u{202f}567"]),
            ("ru", ["999", "1\u{a0}000", "25\u{a0}003", "100\u{a0}000", "1\u{a0}234\u{a0}567"]),
            // CLDR 44's minimum grouping digits: four digits stay together.
            ("es", ["999", "1000", "25.003", "100.000", "1.234.567"]),
            ("pl", ["999", "1000", "25\u{a0}003", "100\u{a0}000", "1\u{a0}234\u{a0}567"]),
            // Later CLDR keeps Italian's four digits together too; Electron 33's does not.
            ("it", ["999", "1.000", "25.003", "100.000", "1.234.567"]),
            ("hi", ["999", "1,000", "25,003", "1,00,000", "12,34,567"]),
            ("bn", ["৯৯৯", "১,০০০", "২৫,০০৩", "১,০০,০০০", "১২,৩৪,৫৬৭"]),
            ("fa", ["۹۹۹", "۱٬۰۰۰", "۲۵٬۰۰۳", "۱۰۰٬۰۰۰", "۱٬۲۳۴٬۵۶۷"]),
            ("ar", ["999", "1,000", "25,003", "100,000", "1,234,567"]),
        ];
        for (app, texts) in cases {
            assert_eq!([999, 1_000, 25_003, 100_000, 1_234_567].map(|n| locale(app).number(n)), texts, "{app}");
        }
        assert_eq!(locale("de").number(0), "0");
        assert_eq!(locale("mr").number(7), "७");
    }

    #[test]
    fn every_locale_formats_every_hour() {
        for locale in ELECTRON_LOCALES {
            assert!(locale.icu.parse::<Locale>().is_ok(), "{}", locale.icu);
            let twelve_hour = matches!(locale.day_periods, DayPeriods::AmPm(..));
            assert_eq!(locale.time_two_digit.contains("{a}"), twelve_hour, "{}", locale.app);
            assert_eq!(locale.time.contains("{a}"), twelve_hour, "{}", locale.app);
            for hour in 0..24 {
                let t = at(2026, 3, 25, hour, 0, 0);
                assert!(locale.date_time(t).is_some(), "{}", locale.app);
                assert!(locale.date(t).is_some(), "{}", locale.app);
                assert!(!locale.time_two_digit(t).is_empty(), "{}", locale.app);
                assert!(!locale.time(t).is_empty(), "{}", locale.app);
            }
        }
    }

    #[test]
    fn strings_compare_as_locale_compare_does() {
        // Punctuation before digits before letters; accents beside their letter; lowercase first.
        assert_eq!(sorted("en-US", &["review.rs", "review_tree.rs"]), ["review_tree.rs", "review.rs"]);
        assert_eq!(sorted("en-US", &["1x", "_x"]), ["_x", "1x"]);
        assert_eq!(sorted("en-US", &["eb", "éa", "ea", "zed", "fa"]), ["ea", "éa", "eb", "fa", "zed"]);
        assert_eq!(sorted("en-US", &["B", "a", "A", "b"]), ["a", "A", "b", "B"]);
        assert_eq!(sorted("en-US", &["src1/a.ts", "src/a.ts", "src-x/a.ts", "src_old/a.ts"]), [
            "src_old/a.ts",
            "src-x/a.ts",
            "src/a.ts",
            "src1/a.ts"
        ]);
        assert_eq!(collator(locale("en-US")).compare("a", "a"), Ordering::Equal);
        // Locales tailor the order: Swedish puts ä after z, Danish uppercase first, Thai ignores
        // punctuation.
        assert_eq!(sorted("sv", &["ä", "z", "a"]), ["a", "z", "ä"]);
        assert_eq!(sorted("da", &["a", "A", "b"]), ["A", "a", "b"]);
        assert_eq!(sorted("th", &["review.rs", "review_tree.rs"]), ["review.rs", "review_tree.rs"]);
    }

    #[test]
    fn compare_uses_the_current_locale() {
        let collator = collator(ElectronLocale::current());
        for (a, b) in [("review.rs", "review_tree.rs"), ("a", "A"), ("éa", "fa"), ("x", "x")] {
            assert_eq!(compare(a, b), collator.compare(a, b), "{a} {b}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_picks_the_localization_nsbundle_picks_for_electron() {
        // Checked against `app.getLocale()` with `-AppleLanguages '(…)'`.
        let cases: [(&'static [&'static str], &str); 13] = [
            (&["en-MY", "en", "fr"], "en-US"),
            (&["en-MY"], "en-GB"),
            (&["en-AU"], "en-GB"),
            (&["en-AU", "en"], "en-US"),
            (&["en-CA"], "en-US"),
            (&["en-GB", "en"], "en-GB"),
            (&["de-AT"], "de"),
            (&["es-AR"], "es-419"),
            (&["pt", "en"], "pt-BR"),
            (&["zh-Hant-HK"], "zh-TW"),
            (&["nn"], "nb"),
            (&["kk"], "en-US"),
            (&["xx", "de"], "de"),
        ];
        for (preferred, app) in cases {
            assert_eq!(bundle_localization(Preferences::Given(preferred)).app, app, "{preferred:?}");
        }
        assert_eq!(bundle_localization(Preferences::Given(&[])).app, "en-US");
        assert_eq!(ElectronLocale::current(), bundle_localization(Preferences::User));
    }

    #[test]
    fn elsewhere_the_first_language_chromium_resolves_wins() {
        let resolve = |vars: &[(&str, &str)]| {
            let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
            resolve_from_env(&|name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())).app
        };
        assert_eq!(resolve(&[("LANG", "de_AT.UTF-8")]), "de");
        assert_eq!(resolve(&[("LANG", "en_GB.UTF-8")]), "en-GB");
        assert_eq!(resolve(&[("LANG", "en_AU.UTF-8")]), "en-GB");
        assert_eq!(resolve(&[("LANG", "en_MY.UTF-8")]), "en-US");
        assert_eq!(resolve(&[("LANG", "es_ES.UTF-8")]), "es");
        assert_eq!(resolve(&[("LANG", "es_AR.UTF-8")]), "es-419");
        assert_eq!(resolve(&[("LANG", "pt_AO")]), "pt-PT");
        assert_eq!(resolve(&[("LANG", "pt")]), "pt-BR");
        assert_eq!(resolve(&[("LANG", "zh_HK.UTF-8")]), "zh-TW");
        assert_eq!(resolve(&[("LANG", "zh_SG")]), "zh-CN");
        assert_eq!(resolve(&[("LANG", "no_NO")]), "nb");
        assert_eq!(resolve(&[("LANG", "iw_IL")]), "he");
        assert_eq!(resolve(&[("LANG", "tl")]), "fil");
        assert_eq!(resolve(&[("LANG", "ca_ES@valencia")]), "ca");
        // Precedence: LANGUAGE (a list), LC_ALL, LC_MESSAGES, LANG; empty values do not count.
        assert_eq!(resolve(&[("LANGUAGE", "kk:fr_CA"), ("LANG", "de_DE")]), "fr");
        assert_eq!(resolve(&[("LANGUAGE", ""), ("LC_ALL", "ja_JP.UTF-8"), ("LANG", "de_DE")]), "ja");
        assert_eq!(resolve(&[("LC_MESSAGES", "sv_SE"), ("LANG", "de_DE")]), "sv");
        // No language Electron ships, the C locale or nothing at all: en-US.
        assert_eq!(resolve(&[("LANG", "kk_KZ")]), "en-US");
        assert_eq!(resolve(&[("LANG", "C.UTF-8")]), "en-US");
        assert_eq!(resolve(&[]), "en-US");
    }
}
