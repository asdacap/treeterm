//! Wall-clock times as the user reads them, in the local time zone: fixed `HH:MM:SS` formats,
//! and what Electron's `toLocaleString` and friends print in its locale ([`crate::locale`]).

use std::time::{SystemTime, UNIX_EPOCH};

use crate::locale::ElectronLocale;

/// Calendar fields of a moment in the local time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    /// 1–12.
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// `at` in the local time zone; `None` when the C library cannot convert it.
pub fn local(at: SystemTime) -> Option<LocalTime> {
    let seconds = match at.duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_secs()).ok()?,
        Err(before) => -i64::try_from(before.duration().as_secs()).ok()?,
    };
    // `time_t` is narrower than i64 on some targets. On musl the alias is deprecated only
    // because its width will change; `try_into` follows whatever width it has.
    #[allow(clippy::useless_conversion, deprecated)]
    let time: libc::time_t = seconds.try_into().ok()?;
    // SAFETY: `localtime_r` writes only into the `tm` we own and reads `time`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let converted = unsafe { libc::localtime_r(&time, &mut tm) };
    if converted.is_null() {
        return None;
    }
    Some(LocalTime {
        year: tm.tm_year + 1900,
        month: u32::try_from(tm.tm_mon + 1).ok()?,
        day: u32::try_from(tm.tm_mday).ok()?,
        hour: u32::try_from(tm.tm_hour).ok()?,
        minute: u32::try_from(tm.tm_min).ok()?,
        second: u32::try_from(tm.tm_sec).ok()?,
    })
}

/// `HH:MM:SS` local time; `?` when it cannot be converted.
pub fn time_of_day(at: SystemTime) -> String {
    match local(at) {
        Some(t) => format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second),
        None => "?".to_owned(),
    }
}

/// `YYYY-MM-DD HH:MM:SS` local time; `?` when it cannot be converted.
pub fn date_time(at: SystemTime) -> String {
    match local(at) {
        Some(t) => format!(
            "{}-{:02}-{:02} {:02}:{:02}:{:02}",
            t.year, t.month, t.day, t.hour, t.minute, t.second
        ),
        None => "?".to_owned(),
    }
}

/// `new Date(millis).toLocaleString()` in Electron's locale (en-US: `11/11/2026, 5:57:12 PM`);
/// `?` when it cannot be converted.
pub fn locale_date_time(millis: i64) -> String {
    match local(from_millis(millis)).and_then(|t| ElectronLocale::current().date_time(t)) {
        Some(text) => text,
        None => "?".to_owned(),
    }
}

/// `new Date(millis).toLocaleDateString()` in Electron's locale (en-US: `11/11/2026`); `?` when
/// it cannot be converted.
pub fn locale_date(millis: i64) -> String {
    match local(from_millis(millis)).and_then(|t| ElectronLocale::current().date(t)) {
        Some(text) => text,
        None => "?".to_owned(),
    }
}

/// `new Date(…).toLocaleTimeString()` in Electron's locale (en-US: `5:57:12 PM`, en-GB:
/// `17:57:12`); `?` when it cannot be converted.
pub fn locale_time(at: SystemTime) -> String {
    match local(at) {
        Some(t) => ElectronLocale::current().time(t),
        None => "?".to_owned(),
    }
}

/// `toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })` in
/// Electron's locale (en-US: `05:57:12 PM`, en-GB: `17:57:12`); `?` when it cannot be converted.
pub fn locale_time_two_digit(at: SystemTime) -> String {
    match local(at) {
        Some(t) => ElectronLocale::current().time_two_digit(t),
        None => "?".to_owned(),
    }
}

/// A JavaScript timestamp (milliseconds since the epoch) as a [`SystemTime`].
pub fn from_millis(millis: i64) -> SystemTime {
    match u64::try_from(millis) {
        Ok(after) => UNIX_EPOCH + std::time::Duration::from_millis(after),
        Err(_) => UNIX_EPOCH - std::time::Duration::from_millis(millis.unsigned_abs()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_times_are_zero_padded_and_round_trip_the_clock() {
        let now = SystemTime::now();
        let t = local(now).unwrap();
        assert!((1..=12).contains(&t.month) && (1..=31).contains(&t.day));
        let text = time_of_day(now);
        assert_eq!(text.len(), 8, "{text}");
        assert_eq!(text, format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second));
        let full = date_time(now);
        assert!(full.ends_with(&text), "{full}");
        assert!(full.starts_with(&t.year.to_string()));
    }

    #[test]
    fn locale_times_are_electrons_for_the_local_time() {
        let epoch = local(UNIX_EPOCH).unwrap();
        let locale = ElectronLocale::current();
        assert_eq!(Some(locale_date_time(0)), locale.date_time(epoch));
        assert_eq!(Some(locale_date(0)), locale.date(epoch));
        assert_eq!(locale_time_two_digit(UNIX_EPOCH), locale.time_two_digit(epoch));
        assert_eq!(locale_time(UNIX_EPOCH), locale.time(epoch));
        assert_ne!(locale_date_time(1_763_315_832_000), "?");
    }

    #[test]
    fn javascript_timestamps_convert_both_ways_of_the_epoch() {
        assert_eq!(from_millis(1_500), UNIX_EPOCH + std::time::Duration::from_millis(1_500));
        assert_eq!(from_millis(-1_000), UNIX_EPOCH - std::time::Duration::from_secs(1));
    }
}
