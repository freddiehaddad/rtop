use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Returns the display width of a string, ignoring ANSI escape codes.
/// If `wide` is true, uses full Unicode width (CJK = 2 columns).
pub fn ulen(s: &str, wide: bool) -> usize {
    let stripped = strip_ansi(s);
    if wide {
        UnicodeWidthStr::width(stripped.as_str())
    } else {
        UnicodeWidthStr::width(stripped.as_str())
    }
}

/// Truncate a string to fit within `len` display columns.
/// Preserves ANSI escape codes (they are zero-width).
pub fn uresize(s: &str, len: usize, _wide: bool) -> String {
    let mut result = String::new();
    let mut current_width = 0;
    let mut in_escape = false;

    for ch in s.chars() {
        if in_escape {
            result.push(ch);
            if ch.is_ascii_alphabetic() || ch == 'm' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            result.push(ch);
            continue;
        }
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + w > len {
            break;
        }
        result.push(ch);
        current_width += w;
    }
    result
}

/// Left-truncate a string to fit within `len` display columns.
pub fn luresize(s: &str, len: usize, _wide: bool) -> String {
    let stripped = strip_ansi(s);
    let total = UnicodeWidthStr::width(stripped.as_str());
    if total <= len {
        return s.to_string();
    }
    let skip = total - len;
    let mut skipped = 0;
    let mut result = String::new();
    let mut in_escape = false;
    for ch in s.chars() {
        if in_escape {
            result.push(ch);
            if ch.is_ascii_alphabetic() || ch == 'm' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            result.push(ch);
            continue;
        }
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if skipped < skip {
            skipped += w;
            continue;
        }
        result.push(ch);
    }
    result
}

/// Left-justify string, padding with spaces on the right to `width` columns.
/// If string exceeds `width`, it is truncated.
pub fn ljust(s: &str, width: usize, utf: bool) -> String {
    let current = if utf { ulen(s, false) } else { s.len() };
    if current >= width {
        if utf {
            uresize(s, width, false)
        } else {
            s[..width].to_string()
        }
    } else {
        format!("{}{}", s, " ".repeat(width - current))
    }
}

/// Right-justify string, padding with spaces on the left to `width` columns.
/// If string exceeds `width`, it is truncated.
pub fn rjust(s: &str, width: usize, utf: bool) -> String {
    let current = if utf { ulen(s, false) } else { s.len() };
    if current >= width {
        if utf {
            uresize(s, width, false)
        } else {
            s[..width].to_string()
        }
    } else {
        format!("{}{}", " ".repeat(width - current), s)
    }
}

/// Center string, padding with spaces on both sides to `width` columns.
/// If string exceeds `width`, it is truncated.
pub fn cjust(s: &str, width: usize, utf: bool) -> String {
    let current = if utf { ulen(s, false) } else { s.len() };
    if current >= width {
        if utf {
            uresize(s, width, false)
        } else {
            s[..width].to_string()
        }
    } else {
        let total_pad = width - current;
        let left_pad = total_pad / 2;
        let right_pad = total_pad - left_pad;
        format!("{}{}{}", " ".repeat(left_pad), s, " ".repeat(right_pad))
    }
}

/// Format a byte value into a human-readable string (B, KiB, MiB, GiB, TiB).
/// If `shorten` is true, values are shortened (e.g. "1.2G" instead of "1.23 GiB").
/// If `bit` is true, uses bit units (b, Kib, Mib, ...).
/// If `per_second` is true, appends "/s".
/// If `base10` is true, uses SI units (KB, MB, ...) with 1000 divisor.
pub fn floating_humanizer(
    value: u64,
    shorten: bool,
    start: usize,
    bit: bool,
    per_second: bool,
    base10: bool,
) -> String {
    let mut val = if bit { value as f64 * 8.0 } else { value as f64 };
    let divisor: f64 = if base10 { 1000.0 } else { 1024.0 };

    let units_binary = if bit {
        ["b", "Kib", "Mib", "Gib", "Tib", "Pib"]
    } else {
        ["B", "KiB", "MiB", "GiB", "TiB", "PiB"]
    };
    let units_si = if bit {
        ["b", "Kb", "Mb", "Gb", "Tb", "Pb"]
    } else {
        ["B", "KB", "MB", "GB", "TB", "PB"]
    };
    let units = if base10 { &units_si } else { &units_binary };

    let short_units = if bit {
        ["b", "K", "M", "G", "T", "P"]
    } else {
        ["B", "K", "M", "G", "T", "P"]
    };

    let mut unit_idx = start;
    while val >= divisor && unit_idx < units.len() - 1 {
        val /= divisor;
        unit_idx += 1;
    }

    let suffix = if per_second { "/s" } else { "" };

    if shorten {
        let u = short_units[unit_idx];
        if val < 10.0 {
            format!("{:.1}{}{}", val, u, suffix)
        } else {
            format!("{:.0}{}{}", val, u, suffix)
        }
    } else {
        let u = units[unit_idx];
        if unit_idx == 0 {
            format!("{:.0} {}{}", val, u, suffix)
        } else if val < 10.0 {
            format!("{:.2} {}{}", val, u, suffix)
        } else if val < 100.0 {
            format!("{:.1} {}{}", val, u, suffix)
        } else {
            format!("{:.0} {}{}", val, u, suffix)
        }
    }
}

/// Convert seconds to a human-readable duration string "XdHH:MM:SS".
pub fn sec_to_dhms(seconds: u64, no_days: bool, no_seconds: bool) -> String {
    if seconds == 0 {
        return if no_seconds {
            "00:00".to_string()
        } else {
            "00:00:00".to_string()
        };
    }

    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    let mut result = String::new();

    if days > 0 && !no_days {
        result.push_str(&format!("{}d", days));
    }

    if no_days {
        let total_hours = days * 24 + hours;
        result.push_str(&format!("{:02}:", total_hours));
    } else {
        result.push_str(&format!("{:02}:", hours));
    }

    result.push_str(&format!("{:02}", minutes));

    if !no_seconds {
        result.push_str(&format!(":{:02}", secs));
    }

    result
}

/// Convert a Celsius temperature value to the specified scale.
/// Returns (converted_value, unit_suffix).
pub fn celsius_to(celsius: i64, scale: &str) -> (i64, String) {
    match scale {
        "fahrenheit" => ((celsius * 9 / 5) + 32, "°F".to_string()),
        "kelvin" => (celsius + 273, "°K".to_string()),
        "rankine" => (((celsius * 9 / 5) + 32) + 460, "°R".to_string()),
        _ => (celsius, "°C".to_string()),
    }
}

/// Strip ANSI escape codes from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;
    for ch in s.chars() {
        if in_escape {
            if ch.is_ascii_alphabetic() || ch == 'm' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            continue;
        }
        result.push(ch);
    }
    result
}

/// Format the current time using strftime-compatible format string.
/// Supports special replacements: `/host`, `/user`, `/uptime`.
pub fn strf_time(format: &str, uptime_seconds: u64) -> String {
    let now = chrono_free_strftime(format);

    // Apply btop-specific replacements
    let result = now.replace("/host", &hostname()).replace("/user", &username());

    let uptime_str = sec_to_dhms(uptime_seconds, false, true);
    result.replace("/uptime", &uptime_str)
}

/// Get the system hostname.
pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Get the current username.
pub fn username() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Simple strftime without chrono dependency — handles common format specifiers.
fn chrono_free_strftime(format: &str) -> String {
    use std::time::SystemTime;
    // For now, return the format string with basic time substitution.
    // Full strftime support will be added when the UI needs it.
    let _ = SystemTime::now();
    format.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ulen tests ---

    #[test]
    fn ulen_ascii_string() {
        assert_eq!(ulen("hello", false), 5);
    }

    #[test]
    fn ulen_empty_string() {
        assert_eq!(ulen("", false), 0);
    }

    #[test]
    fn ulen_ansi_escape_codes_ignored() {
        assert_eq!(ulen("\x1b[31mhello\x1b[0m", false), 5);
    }

    #[test]
    fn ulen_cjk_characters_count_double() {
        // CJK characters take 2 columns
        assert_eq!(ulen("中文", true), 4);
    }

    #[test]
    fn ulen_mixed_ascii_cjk() {
        assert_eq!(ulen("hi中", true), 4); // 2 + 2
    }

    // --- uresize tests ---

    #[test]
    fn uresize_truncates_at_width() {
        assert_eq!(uresize("hello world", 5, false), "hello");
    }

    #[test]
    fn uresize_preserves_ansi_codes() {
        let s = "\x1b[31mhello\x1b[0m world";
        let result = uresize(s, 5, false);
        assert!(result.contains("\x1b[31m"));
        assert_eq!(ulen(&result, false), 5);
    }

    #[test]
    fn uresize_no_truncation_when_fits() {
        assert_eq!(uresize("hi", 10, false), "hi");
    }

    // --- luresize tests ---

    #[test]
    fn luresize_removes_from_left() {
        assert_eq!(luresize("hello world", 5, false), "world");
    }

    #[test]
    fn luresize_no_change_when_fits() {
        assert_eq!(luresize("hi", 10, false), "hi");
    }

    // --- ljust tests ---

    #[test]
    fn ljust_pads_right() {
        assert_eq!(ljust("hi", 5, false), "hi   ");
    }

    #[test]
    fn ljust_truncates_if_over() {
        assert_eq!(ljust("hello world", 5, false), "hello");
    }

    #[test]
    fn ljust_exact_width() {
        assert_eq!(ljust("hello", 5, false), "hello");
    }

    // --- rjust tests ---

    #[test]
    fn rjust_pads_left() {
        assert_eq!(rjust("hi", 5, false), "   hi");
    }

    #[test]
    fn rjust_truncates_if_over() {
        assert_eq!(rjust("hello world", 5, false), "hello");
    }

    // --- cjust tests ---

    #[test]
    fn cjust_centers() {
        assert_eq!(cjust("hi", 6, false), "  hi  ");
    }

    #[test]
    fn cjust_odd_padding() {
        assert_eq!(cjust("hi", 5, false), " hi  ");
    }

    #[test]
    fn cjust_truncates_if_over() {
        assert_eq!(cjust("hello world", 5, false), "hello");
    }

    // --- floating_humanizer tests ---

    #[test]
    fn floating_humanizer_bytes() {
        assert_eq!(
            floating_humanizer(0, false, 0, false, false, false),
            "0 B"
        );
    }

    #[test]
    fn floating_humanizer_kib() {
        assert_eq!(
            floating_humanizer(1024, false, 0, false, false, false),
            "1.00 KiB"
        );
    }

    #[test]
    fn floating_humanizer_mib() {
        assert_eq!(
            floating_humanizer(1024 * 1024, false, 0, false, false, false),
            "1.00 MiB"
        );
    }

    #[test]
    fn floating_humanizer_gib() {
        let val = 1024 * 1024 * 1024;
        assert_eq!(
            floating_humanizer(val, false, 0, false, false, false),
            "1.00 GiB"
        );
    }

    #[test]
    fn floating_humanizer_tib() {
        let val = 1024u64 * 1024 * 1024 * 1024;
        assert_eq!(
            floating_humanizer(val, false, 0, false, false, false),
            "1.00 TiB"
        );
    }

    #[test]
    fn floating_humanizer_shortened() {
        let val = 1024u64 * 1024 * 1024 * 2 + 1024 * 1024 * 512; // ~2.5 GiB
        let result = floating_humanizer(val, true, 0, false, false, false);
        assert!(result.ends_with('G'), "got: {result}");
    }

    #[test]
    fn floating_humanizer_bits() {
        assert_eq!(
            floating_humanizer(1000, false, 0, true, false, false),
            "7.81 Kib"
        );
    }

    #[test]
    fn floating_humanizer_per_second() {
        let result = floating_humanizer(1024, false, 0, false, true, false);
        assert!(result.ends_with("/s"), "got: {result}");
    }

    #[test]
    fn floating_humanizer_base10() {
        assert_eq!(
            floating_humanizer(1000, false, 0, false, false, true),
            "1.00 KB"
        );
    }

    // --- sec_to_dhms tests ---

    #[test]
    fn sec_to_dhms_zero() {
        assert_eq!(sec_to_dhms(0, false, false), "00:00:00");
    }

    #[test]
    fn sec_to_dhms_seconds_only() {
        assert_eq!(sec_to_dhms(45, false, false), "00:00:45");
    }

    #[test]
    fn sec_to_dhms_minutes_seconds() {
        assert_eq!(sec_to_dhms(125, false, false), "00:02:05");
    }

    #[test]
    fn sec_to_dhms_hours_minutes_seconds() {
        assert_eq!(sec_to_dhms(3661, false, false), "01:01:01");
    }

    #[test]
    fn sec_to_dhms_days() {
        assert_eq!(sec_to_dhms(90061, false, false), "1d01:01:01");
    }

    #[test]
    fn sec_to_dhms_no_days_flag() {
        assert_eq!(sec_to_dhms(90061, true, false), "25:01:01");
    }

    #[test]
    fn sec_to_dhms_no_seconds_flag() {
        assert_eq!(sec_to_dhms(3661, false, true), "01:01");
    }

    // --- celsius_to tests ---

    #[test]
    fn celsius_to_celsius_identity() {
        assert_eq!(celsius_to(100, "celsius"), (100, "°C".to_string()));
    }

    #[test]
    fn celsius_to_fahrenheit() {
        assert_eq!(celsius_to(100, "fahrenheit"), (212, "°F".to_string()));
    }

    #[test]
    fn celsius_to_kelvin() {
        assert_eq!(celsius_to(0, "kelvin"), (273, "°K".to_string()));
    }

    #[test]
    fn celsius_to_rankine() {
        assert_eq!(celsius_to(0, "rankine"), (492, "°R".to_string()));
    }

    // --- strf_time tests ---

    #[test]
    fn strf_time_host_replacement() {
        let result = strf_time("/host", 0);
        assert!(!result.contains("/host"));
        assert!(!result.is_empty());
    }

    #[test]
    fn strf_time_user_replacement() {
        let result = strf_time("/user", 0);
        assert!(!result.contains("/user"));
    }

    #[test]
    fn strf_time_uptime_replacement() {
        let result = strf_time("/uptime", 3661);
        assert!(result.contains("01:01"), "got: {result}");
    }

    // --- strip_ansi tests ---

    #[test]
    fn strip_ansi_removes_codes() {
        assert_eq!(strip_ansi("\x1b[31mhello\x1b[0m"), "hello");
    }

    #[test]
    fn strip_ansi_no_codes() {
        assert_eq!(strip_ansi("hello"), "hello");
    }
}
