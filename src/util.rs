use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tmux_tools_core::agents::Registry;

/// The current UTC time as RFC 3339 with millisecond precision
/// (`2026-11-09T11:24:30.123Z`), so two launches less than a second apart still order
/// (PRD-261108-0447-01 P8.3).
pub fn rfc3339_utc_now() -> Result<String> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)?;
    let seconds = format_unix_utc(elapsed.as_secs());
    let stem = seconds.trim_end_matches('Z');
    Ok(format!("{stem}.{:03}Z", elapsed.subsec_millis()))
}

/// An RFC 3339 UTC instant as whole seconds since the epoch and, when the text has a
/// fraction, its nanoseconds. Accepts `Z` and `+00:00`; `None` for any other text.
pub fn parse_rfc3339_utc(text: &str) -> Option<(i64, Option<u32>)> {
    let text = text
        .strip_suffix('Z')
        .or_else(|| text.strip_suffix("+00:00"))?;
    let (date, time) = text.split_once('T')?;
    let mut date_parts = date.splitn(3, '-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    let (clock, fraction) = match time.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (time, None),
    };
    let mut clock_parts = clock.splitn(3, ':');
    let hour: i64 = clock_parts.next()?.parse().ok()?;
    let minute: i64 = clock_parts.next()?.parse().ok()?;
    let second: i64 = clock_parts.next()?.parse().ok()?;
    let nanos = match fraction {
        None => None,
        Some(digits) if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => {
            let padded = format!("{:0<9}", &digits[..digits.len().min(9)]);
            Some(padded.parse().ok()?)
        }
        Some(_) => return None,
    };
    let days = days_from_civil(year, month, day);
    Some((days * 86_400 + hour * 3_600 + minute * 60 + second, nanos))
}

/// Whether instant `a` is later than instant `b` (both [`parse_rfc3339_utc`] results).
/// When either has whole-second precision only, the seconds decide, so a whole-second
/// record is never later than a sub-second record of the same second, nor the reverse.
pub fn instant_later(a: (i64, Option<u32>), b: (i64, Option<u32>)) -> bool {
    match (a, b) {
        ((a_secs, Some(a_nanos)), (b_secs, Some(b_nanos))) => (a_secs, a_nanos) > (b_secs, b_nanos),
        ((a_secs, _), (b_secs, _)) => a_secs > b_secs,
    }
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

pub fn format_unix_utc(total_seconds: u64) -> String {
    let days = (total_seconds / 86_400) as i64;
    let seconds_of_day = total_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year_of_era + era * 400 + if month <= 2 { 1 } else { 0 };

    (year as i32, month as u32, day as u32)
}

/// Load the agent registry for a verb that only needs its floors (`launch`, `status`).
///
/// Per-entry load warnings go to stderr as `spawn-agent` prints them. A user
/// `agents.toml` that exists but cannot be read or parsed does not fail the verb: one
/// warning line goes to stderr and the built-in registry is used alone.
pub fn load_registry_for_floors() -> Registry {
    let (registry, warnings, failure) = Registry::load_or_builtin();
    if let Some(failure) = failure {
        eprintln!(
            "warning: agent registry {}: {}; using built-in floors",
            failure.path.display(),
            failure.error
        );
    }
    for warning in &warnings {
        eprintln!("warning: agent {}: {}", warning.agent, warning.detail);
    }
    registry
}

#[cfg(test)]
mod instant_tests {
    use super::*;

    #[test]
    fn whole_and_sub_second_instants_parse_and_order() {
        let whole = parse_rfc3339_utc("2026-11-09T11:24:30Z").unwrap();
        let early = parse_rfc3339_utc("2026-11-09T11:24:30.100Z").unwrap();
        let late = parse_rfc3339_utc("2026-11-09T11:24:30.900Z").unwrap();
        let next = parse_rfc3339_utc("2026-11-09T11:24:31Z").unwrap();
        assert_eq!(format_unix_utc(whole.0 as u64), "2026-11-09T11:24:30Z");
        assert!(instant_later(late, early) && !instant_later(early, late));
        // A whole-second record and a sub-second record of the same second: neither is
        // later.
        assert!(!instant_later(late, whole) && !instant_later(whole, late));
        assert!(instant_later(next, late) && !instant_later(early, next));
        assert_eq!(parse_rfc3339_utc("not a time"), None);
        let now = rfc3339_utc_now().unwrap();
        assert!(parse_rfc3339_utc(&now).unwrap().1.is_some(), "{now}");
    }
}
