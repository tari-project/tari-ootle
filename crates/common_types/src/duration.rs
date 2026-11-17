// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

/// Formats a duration as a compact human-readable string, e.g. `1h 2m 3s` or `45s`.
/// Sub-second durations (or durations with fractional seconds) preserve milliseconds.
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    if secs == 0 {
        return format!("{}ms", millis);
    }
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    let s = secs % 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if mins > 0 {
        parts.push(format!("{mins}m"));
    }
    if s > 0 || (parts.is_empty() && millis == 0) {
        parts.push(format!("{s}s"));
    }
    if millis > 0 {
        parts.push(format!("{millis}ms"));
    }
    parts.join(" ")
}

/// Parses a human-friendly duration such as `30d`, `12h`, `1y`, `90 days` or `1day 6h` into a [`Duration`].
/// Units: `ns`, `us`, `ms`, `s`, `m` (minutes), `h`, `d`, `w`, `M` (months, 30.44 days), `y` (365.25 days).
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    const UNITS: &[(&str, u64)] = &[
        ("nsec", 1),
        ("ns", 1),
        ("usec", 1_000),
        ("us", 1_000),
        ("msec", 1_000_000),
        ("ms", 1_000_000),
        ("seconds", 1_000_000_000),
        ("second", 1_000_000_000),
        ("secs", 1_000_000_000),
        ("sec", 1_000_000_000),
        ("s", 1_000_000_000),
        ("minutes", 60_000_000_000),
        ("minute", 60_000_000_000),
        ("mins", 60_000_000_000),
        ("min", 60_000_000_000),
        ("m", 60_000_000_000),
        ("hours", 3_600_000_000_000),
        ("hour", 3_600_000_000_000),
        ("hrs", 3_600_000_000_000),
        ("hr", 3_600_000_000_000),
        ("h", 3_600_000_000_000),
        ("days", 86_400_000_000_000),
        ("day", 86_400_000_000_000),
        ("d", 86_400_000_000_000),
        ("weeks", 604_800_000_000_000),
        ("week", 604_800_000_000_000),
        ("w", 604_800_000_000_000),
        ("months", 2_630_016_000_000_000),
        ("month", 2_630_016_000_000_000),
        ("M", 2_630_016_000_000_000),
        ("years", 31_557_600_000_000_000),
        ("year", 31_557_600_000_000_000),
        ("y", 31_557_600_000_000_000),
    ];

    let mut nanos: u128 = 0;
    let mut rest = s.trim();
    if rest.is_empty() {
        return Err("empty duration".to_string());
    }
    while !rest.is_empty() {
        let num_len = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        if num_len == 0 {
            return Err(format!("expected number at '{rest}'"));
        }
        let (num_str, after_num) = rest.split_at(num_len);
        let value: f64 = num_str
            .parse()
            .map_err(|_| format!("invalid number '{num_str}'"))?;
        rest = after_num.trim_start();
        if rest.is_empty() {
            return Err(format!("number '{num_str}' is missing a duration unit"));
        }
        let unit_len = rest
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(rest.len());
        let (unit, after_unit) = rest.split_at(unit_len);
        let unit = unit.trim();
        let Some(&(_, nanos_per_unit)) = UNITS.iter().find(|(name, _)| *name == unit) else {
            return Err(format!("unknown unit '{unit}'"));
        };
        let segment_nanos = (value * nanos_per_unit as f64) as u128;
        nanos = nanos
            .checked_add(segment_nanos)
            .ok_or_else(|| "duration overflow".to_string())?;
        rest = after_unit.trim_start();
    }
    Ok(Duration::from_nanos(
        u64::try_from(nanos).map_err(|_| "duration overflow".to_string())?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_valid_forms() {
        assert_eq!(parse_duration("30d").unwrap(), Duration::from_secs(30 * 86_400));
        assert_eq!(
            parse_duration("1h30m").unwrap(),
            Duration::from_secs(3_600 + 30 * 60)
        );
        assert_eq!(
            parse_duration("1h 30m").unwrap(),
            Duration::from_secs(3_600 + 30 * 60)
        );
        assert_eq!(
            parse_duration("90 days").unwrap(),
            Duration::from_secs(90 * 86_400)
        );
        assert_eq!(
            parse_duration("1day 6h").unwrap(),
            Duration::from_secs(86_400 + 6 * 3_600)
        );
    }

    #[test]
    fn test_parse_duration_error_cases() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("   ").is_err());
        assert!(parse_duration("30").is_err());
        assert!(parse_duration("5x").is_err());
        assert!(parse_duration("abc").is_err());
    }

    #[test]
    fn test_parse_duration_overflow() {
        assert!(parse_duration("9999999999999999999999999999999y").is_err());
        assert!(parse_duration("18446744073709551615s").is_err());
    }

    #[test]
    fn test_format_and_roundtrip() {
        let test_durations = [
            Duration::from_secs(0),
            Duration::from_millis(500),
            Duration::from_millis(1500),
            Duration::from_secs(45),
            Duration::from_secs(3_600 + 30 * 60),
            Duration::from_secs(86_400 * 30),
        ];

        for d in test_durations {
            let formatted = format_duration(d);
            let parsed = parse_duration(&formatted).unwrap();
            assert_eq!(parsed, d, "failed roundtrip for {d:?} (formatted as '{formatted}')");
        }
    }
}
