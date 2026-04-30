use unicode_width::UnicodeWidthStr;

/// Returns the display width of a string, ignoring ANSI escape codes.
/// If `wide` is true, uses full Unicode width (CJK = 2 columns).
pub fn ulen(s: &str, _wide: bool) -> usize {
    let stripped = strip_ansi(s);
    UnicodeWidthStr::width(stripped.as_str())
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

#[cfg(test)]
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

#[cfg(test)]
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

/// Strip ANSI escape codes from a string.
pub(super) fn strip_ansi(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(ulen("中文", true), 4);
    }

    #[test]
    fn ulen_mixed_ascii_cjk() {
        assert_eq!(ulen("hi中", true), 4);
    }

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

    #[test]
    fn luresize_removes_from_left() {
        assert_eq!(luresize("hello world", 5, false), "world");
    }

    #[test]
    fn luresize_no_change_when_fits() {
        assert_eq!(luresize("hi", 10, false), "hi");
    }

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

    #[test]
    fn rjust_pads_left() {
        assert_eq!(rjust("hi", 5, false), "   hi");
    }

    #[test]
    fn rjust_truncates_if_over() {
        assert_eq!(rjust("hello world", 5, false), "hello");
    }

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

    #[test]
    fn strip_ansi_removes_codes() {
        assert_eq!(strip_ansi("\x1b[31mhello\x1b[0m"), "hello");
    }

    #[test]
    fn strip_ansi_no_codes() {
        assert_eq!(strip_ansi("hello"), "hello");
    }
}
