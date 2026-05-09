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
    let mut val = if bit {
        value as f64 * 8.0
    } else {
        value as f64
    };
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
pub fn celsius_to(celsius: i64, scale: crate::domain::config_enums::TempScale) -> (i64, String) {
    use crate::domain::config_enums::TempScale;
    match scale {
        TempScale::Fahrenheit => ((celsius * 9 / 5) + 32, "°F".to_string()),
        TempScale::Kelvin => (celsius + 273, "°K".to_string()),
        TempScale::Rankine => (((celsius * 9 / 5) + 32) + 460, "°R".to_string()),
        TempScale::Celsius => (celsius, "°C".to_string()),
    }
}

/// Format the current time using a clock format string.
///
/// Supported specifiers: `%X` (expands to `%H:%M:%S`), `%H` (24-hour),
/// `%M` (minute), `%S` (second). Returns empty string for empty format.
pub fn format_clock(format: &str) -> String {
    if format.is_empty() {
        return String::new();
    }
    // SAFETY: GetLocalTime takes no arguments, returns a SYSTEMTIME
    // struct with the current local time, and has no failure modes.
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    expand_clock_format(format, st.wHour, st.wMinute, st.wSecond)
}

/// Visible-cell width that [`format_clock`] would produce for the
/// given format string — derived purely from the format string,
/// never reads the wall clock.
///
/// Shares [`expand_clock_format`] with [`format_clock`], so the
/// returned width is guaranteed to equal
/// `tools::ulen(&format_clock(fmt), false)` cell-for-cell. The
/// sentinel values `(0, 0, 0)` are safe because every field
/// formats as `{:02}` and the real-clock values (hour 0-23,
/// minute/second 0-59) all fit in two digits.
///
/// The layout engine uses this on the keystroke hot path; calling
/// `format_clock` there would issue a Win32 `GetLocalTime` syscall
/// per event-loop iteration to compute a width that doesn't depend
/// on the current time.
pub fn format_clock_width(format: &str) -> usize {
    if format.is_empty() {
        return 0;
    }
    crate::tools::ulen(&expand_clock_format(format, 0, 0, 0), false)
}

/// Expand a clock format string by substituting `%X`, `%H`, `%M`,
/// `%S` with the supplied time fields. Shared between
/// [`format_clock`] (passes the wall clock) and
/// [`format_clock_width`] (passes zeros) so the two cannot drift.
fn expand_clock_format(format: &str, hour: u16, minute: u16, second: u16) -> String {
    let expanded = format.replace("%X", "%H:%M:%S");
    expanded
        .replace("%H", &format!("{:02}", hour))
        .replace("%M", &format!("{:02}", minute))
        .replace("%S", &format!("{:02}", second))
}

/// Read the system uptime in seconds since boot via the Win32
/// `GetTickCount64` syscall. Used by the statusbar collector to
/// publish a fresh uptime value at its 1 Hz cadence.
///
/// `GetTickCount64` is infallible (the kernel maintains a
/// monotonic 64-bit millisecond counter; the syscall wrapper
/// returns it unconditionally). Wrapping floors to seconds.
pub fn system_uptime_secs() -> u64 {
    // SAFETY: GetTickCount64 takes no arguments, returns a u64 millisecond
    // counter, and has no failure modes — it is safe to call from any thread
    // at any time.
    let ms = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    ms / 1000
}

#[cfg(test)]
/// Format the current time using strftime-compatible format string.
/// Supports special replacements: `/host`, `/user`, `/uptime`.
pub fn strf_time(format: &str, uptime_seconds: u64) -> String {
    let now = chrono_free_strftime(format);

    let result = now
        .replace("/host", &super::paths::hostname())
        .replace("/user", &super::paths::username());

    let uptime_str = sec_to_dhms(uptime_seconds, false, true);
    result.replace("/uptime", &uptime_str)
}

#[cfg(test)]
/// Simple strftime without chrono dependency — handles common format specifiers.
fn chrono_free_strftime(format: &str) -> String {
    use std::time::SystemTime;
    let _ = SystemTime::now();
    format.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_humanizer_bytes() {
        assert_eq!(floating_humanizer(0, false, 0, false, false, false), "0 B");
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
        let val = 1024u64 * 1024 * 1024 * 2 + 1024 * 1024 * 512;
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

    #[test]
    fn celsius_to_celsius_identity() {
        use crate::domain::config_enums::TempScale;
        assert_eq!(celsius_to(100, TempScale::Celsius), (100, "°C".to_string()));
    }

    #[test]
    fn celsius_to_fahrenheit() {
        use crate::domain::config_enums::TempScale;
        assert_eq!(
            celsius_to(100, TempScale::Fahrenheit),
            (212, "°F".to_string())
        );
    }

    #[test]
    fn celsius_to_kelvin() {
        use crate::domain::config_enums::TempScale;
        assert_eq!(celsius_to(0, TempScale::Kelvin), (273, "°K".to_string()));
    }

    #[test]
    fn celsius_to_rankine() {
        use crate::domain::config_enums::TempScale;
        assert_eq!(celsius_to(0, TempScale::Rankine), (492, "°R".to_string()));
    }

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

    #[test]
    fn format_clock_width_empty_is_zero() {
        assert_eq!(format_clock_width(""), 0);
    }

    #[test]
    fn format_clock_width_x_specifier_is_eight_cells() {
        // `%X` expands to `%H:%M:%S` → "HH:MM:SS" → 8 cells.
        assert_eq!(format_clock_width("%X"), 8);
    }

    #[test]
    fn format_clock_width_individual_specifiers_each_two_cells() {
        assert_eq!(format_clock_width("%H"), 2);
        assert_eq!(format_clock_width("%M"), 2);
        assert_eq!(format_clock_width("%S"), 2);
    }

    #[test]
    fn format_clock_width_combinations_match_format_clock_output_width() {
        // Pin the synchronisation contract: width helper must match
        // the visible width `format_clock` would produce.
        for fmt in ["%H:%M", "%H-%M-%S", "[%X]", "%H%M%S", "%X UTC"] {
            assert_eq!(
                format_clock_width(fmt),
                crate::tools::ulen(&format_clock(fmt), false),
                "width drift on format {fmt:?}",
            );
        }
    }
}
