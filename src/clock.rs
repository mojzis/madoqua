//! Timestamps for the log, without a date library.
//!
//! The log needs an RFC 3339 timestamp that `madoqua stats` can read back.
//! Local offsets need a timezone database, so madoqua writes UTC with a `Z` —
//! unambiguous, sortable, and comparable across the machines a shared log
//! might collect runs from.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch, right now.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// `2026-08-31T12:03:22Z`.
pub fn format_rfc3339_utc(unix_seconds: i64) -> String {
    let days = unix_seconds.div_euclid(86_400);
    let seconds = unix_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Read back a timestamp this crate wrote, and tolerate one with a numeric
/// offset in case a log was written by a version that emitted local time.
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 19 {
        return None;
    }

    let year: i64 = text.get(0..4)?.parse().ok()?;
    let month: u32 = text.get(5..7)?.parse().ok()?;
    let day: u32 = text.get(8..10)?.parse().ok()?;
    let hour: i64 = text.get(11..13)?.parse().ok()?;
    let minute: i64 = text.get(14..16)?.parse().ok()?;
    let second: i64 = text.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let local = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    Some(local - offset_seconds(&text[19..])?)
}

/// The `Z`, `+HH:MM` or `-HH:MM` tail of a timestamp, in seconds.
fn offset_seconds(tail: &str) -> Option<i64> {
    // Fractional seconds, if any, come before the zone designator.
    let tail = tail.strip_prefix('.').map_or(tail, |rest| {
        rest.find(|c: char| !c.is_ascii_digit()).map_or("", |end| &rest[end..])
    });

    match tail.chars().next() {
        None | Some('Z' | 'z') => Some(0),
        Some(sign @ ('+' | '-')) => {
            let hours: i64 = tail.get(1..3)?.parse().ok()?;
            let minutes: i64 = tail.get(4..6)?.parse().ok()?;
            let magnitude = hours * 3600 + minutes * 60;
            Some(if sign == '-' { -magnitude } else { magnitude })
        }
        Some(_) => None,
    }
}

/// Howard Hinnant's civil-date algorithms, which are exact for any year the
/// calendar is defined for and need no lookup tables.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let (day, month) = (u32::try_from(day).unwrap_or(1), u32::try_from(month).unwrap_or(1));
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_formats_as_the_epoch() {
        assert_eq!(format_rfc3339_utc(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn a_known_instant_formats_correctly() {
        // 2026-08-31T14:03:22+02:00 is 12:03:22 UTC.
        assert_eq!(format_rfc3339_utc(1_788_177_802), "2026-08-31T12:03:22Z");
    }

    #[test]
    fn a_leap_day_survives_the_round_trip() {
        let leap_day = parse_rfc3339("2024-02-29T23:59:59Z").unwrap();
        assert_eq!(format_rfc3339_utc(leap_day), "2024-02-29T23:59:59Z");
    }

    #[test]
    fn formatting_and_parsing_are_inverses_across_a_wide_range() {
        for seconds in [0_i64, 1, 86_399, 86_400, 951_782_400, 1_788_177_802, 4_102_444_800] {
            assert_eq!(
                parse_rfc3339(&format_rfc3339_utc(seconds)),
                Some(seconds),
                "the log has to read back exactly what it wrote, at {seconds}"
            );
        }
    }

    #[test]
    fn an_offset_timestamp_is_normalised_to_utc() {
        assert_eq!(
            parse_rfc3339("2026-08-31T14:03:22+02:00"),
            parse_rfc3339("2026-08-31T12:03:22Z"),
            "the same instant written two ways must compare equal"
        );
        assert_eq!(
            parse_rfc3339("2026-08-31T07:03:22-05:00"),
            parse_rfc3339("2026-08-31T12:03:22Z"),
            "a negative offset moves the other way"
        );
    }

    #[test]
    fn nonsense_is_rejected_rather_than_guessed_at() {
        for text in ["", "yesterday", "2026-08-31", "2026-13-01T00:00:00Z"] {
            assert_eq!(parse_rfc3339(text), None, "`{text}` is not a timestamp");
        }
    }
}
