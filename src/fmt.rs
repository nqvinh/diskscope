//! Formatting helpers: sizes, counts, local dates, durations and CSV fields.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn size(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if b < 1024 {
        return format!("{b} B");
    }
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < U.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", U[u])
}

/// 2428517 -> "2,428,517"
pub fn count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn now() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0)
}

fn offset() -> i64 {
    static OFF: OnceLock<i64> = OnceLock::new();
    *OFF.get_or_init(crate::sys::local_utc_offset_secs)
}

/// Days since 1970-01-01 -> (year, month, day) (Howard Hinnant's civil_from_days).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn parts(unix: u32) -> (i64, u32, u32, i64, i64, i64) {
    let t = unix as i64 + offset();
    let (y, m, d) = civil(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    (y, m, d, s / 3600, (s / 60) % 60, s % 60)
}

/// Local "YYYY-MM-DD HH:MM"; empty for unknown (0).
pub fn date(unix: u32) -> String {
    if unix == 0 {
        return String::new();
    }
    let (y, m, d, h, mi, _) = parts(unix);
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
}

/// Local ISO-8601 timestamp with offset, for logs.
pub fn iso_now() -> String {
    let (y, m, d, h, mi, s) = parts(now());
    let o = offset();
    let sign = if o < 0 { '-' } else { '+' };
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}{sign}{:02}:{:02}", o.abs() / 3600, (o.abs() / 60) % 60)
}

/// "7d", "12h", "2w", "30" (days) -> seconds.
pub fn parse_age(s: &str) -> Option<u32> {
    let s = s.trim().to_lowercase();
    let (num, unit) = match s.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((i, _)) => (&s[..i], &s[i..]),
        None => (s.as_str(), "d"),
    };
    let n: u32 = num.parse().ok()?;
    let mult = match unit {
        "h" => 3600,
        "d" => 86_400,
        "w" => 7 * 86_400,
        "m" | "mo" => 30 * 86_400,
        "y" => 365 * 86_400,
        _ => return None,
    };
    n.checked_mul(mult)
}

pub fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(1536), "1.5 KB");
        assert_eq!(size(403_100_000_000), "375.4 GB");
        assert_eq!(count(2_428_517), "2,428,517");
        assert_eq!(count(12), "12");
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_779), (2026, 11, 22));
        assert_eq!(parse_age("7d"), Some(7 * 86_400));
        assert_eq!(parse_age("12h"), Some(12 * 3600));
        assert_eq!(parse_age("30"), Some(30 * 86_400));
        assert_eq!(parse_age("x"), None);
        assert_eq!(csv_field("a,b"), "\"a,b\"");
    }
}

/// "1 item" / "3 items".
pub fn items(n: usize) -> String {
    if n == 1 { "1 item".into() } else { format!("{} items", count(n as u64)) }
}

/// Local date only, "2026-11-22"; empty for unknown.
pub fn day(unix: u32) -> String {
    date(unix).get(..10).unwrap_or("").to_string()
}

/// "today", "3 days ago", "5 months ago" (for tooltips).
pub fn ago(unix: u32) -> String {
    if unix == 0 {
        return "unknown".into();
    }
    let d = now().saturating_sub(unix) / 86_400;
    match d {
        0 => "today".into(),
        1 => "yesterday".into(),
        2..=59 => format!("{d} days ago"),
        60..=729 => format!("{} months ago", d / 30),
        _ => format!("{} years ago", d / 365),
    }
}
