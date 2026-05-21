//! Post-process pass that scales every truecolor SGR foreground /
//! background escape in an ANSI buffer to a fixed percentage of its
//! original brightness. Used by the central renderer to dim the
//! widget underlay layer beneath an active modal so the modal clearly
//! has focus.
//!
//! The transform is a single forward byte scan that parses CSI
//! sequences per ECMA-48 §5.4 and rewrites only `38;2;R;G;B`
//! (truecolor foreground) and `48;2;R;G;B` (truecolor background)
//! subsequences within SGR (`m`) sequences. Every other escape
//! (cursor positioning, clear-screen, indexed/256-color, bare reset,
//! bold, etc.) and every non-escape byte passes through unchanged.
//!
//! ANSI's SGR 2 ("Dim/Faint") attribute is not used: it is unreliably
//! supported on truecolor escapes (which is what every cell in rtop
//! emits), and it is forward-only — emitting it does not retroactively
//! dim text already on screen. Re-rendering with scaled RGB triples is
//! the only reliable approach.

/// The percentage of original RGB channel brightness that survives
/// the dim pass.
///
/// 35% is dim enough to clearly de-emphasise the underlay layer
/// without losing structural detail. 25% loses detail (graph
/// baselines disappear into the background); 50% leaves the modal
/// still competing for attention.
pub const DIM_SCALE_PERCENT: u8 = 35;

const ESC: u8 = 0x1B;
const CSI_INTRO: u8 = b'[';

/// Scale a single 0..=255 RGB channel by [`DIM_SCALE_PERCENT`].
///
/// Integer math: `c * 35 / 100`. The intermediate `c as u16 * 35`
/// peaks at `255 * 35 = 8925`, well within `u16`, so no overflow.
#[inline]
fn scale_channel(c: u8) -> u8 {
    (c as u16 * DIM_SCALE_PERCENT as u16 / 100) as u8
}

/// Build a truecolor background SGR escape whose RGB triple equals
/// what [`dim_truecolor`] would produce when transforming a
/// `\x1b[48;2;R;G;B m` escape with the given input RGB.
///
/// Used by overlays that want to paint character cells whose
/// background blends seamlessly with the dimmed widget underlay —
/// the two derivations share [`DIM_SCALE_PERCENT`] and [`scale_channel`]
/// so they cannot drift from each other.
pub fn dim_bg_escape(rgb: [u8; 3]) -> String {
    format!(
        "\x1b[48;2;{};{};{}m",
        scale_channel(rgb[0]),
        scale_channel(rgb[1]),
        scale_channel(rgb[2]),
    )
}

/// Walk `input` and return a copy with every truecolor SGR
/// foreground/background escape's RGB triple scaled by
/// [`DIM_SCALE_PERCENT`]. Non-SGR escapes and non-escape text pass
/// through unchanged.
pub fn dim_truecolor(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    let mut text_start = 0usize;
    while i < bytes.len() {
        if bytes[i] != ESC {
            i += 1;
            continue;
        }

        // Flush any pending non-escape text. The slice is guaranteed
        // valid UTF-8 because `input` is `&str` and we have not split
        // any multi-byte sequence (we only ever advance by one when
        // the byte is below 0x80, i.e. ASCII, or when we are past
        // a complete CSI sequence which is itself pure ASCII).
        if text_start < i {
            out.push_str(&input[text_start..i]);
        }

        // Bare ESC (no `[` follows) — emit and continue.
        if i + 1 >= bytes.len() || bytes[i + 1] != CSI_INTRO {
            out.push('\x1b');
            i += 1;
            text_start = i;
            continue;
        }

        // Parameter bytes: 0x30..=0x3F (digits, `;`, `:`, `<`, `=`,
        // `>`, `?`).
        let params_start = i + 2;
        let mut p = params_start;
        while p < bytes.len() && (0x30..=0x3F).contains(&bytes[p]) {
            p += 1;
        }
        // Intermediate bytes: 0x20..=0x2F. rtop does not emit these,
        // but a robust CSI parser must consume them so they do not
        // get misclassified as a final byte.
        let intermediates_start = p;
        while p < bytes.len() && (0x20..=0x2F).contains(&bytes[p]) {
            p += 1;
        }
        // Final byte: 0x40..=0x7E.
        if p >= bytes.len() || !(0x40..=0x7E).contains(&bytes[p]) {
            // Malformed CSI (truncated or invalid final byte). Emit
            // the bare ESC and resume one byte later — the next ASCII
            // byte will be picked up by the text-flush path.
            out.push('\x1b');
            i += 1;
            text_start = i;
            continue;
        }
        let final_byte = bytes[p];
        let params = &bytes[params_start..intermediates_start];
        let intermediates = &bytes[intermediates_start..p];
        let csi_end = p + 1;

        if final_byte == b'm' && intermediates.is_empty() {
            // SGR sequence — try the dim rewrite. All bytes in a
            // SGR parameter list are ASCII, so direct slice push is
            // safe.
            out.push_str(&rewrite_sgr(params));
        } else {
            // Pass through verbatim. CSI sequences are pure ASCII,
            // so the slice is valid UTF-8.
            out.push_str(&input[i..csi_end]);
        }
        i = csi_end;
        text_start = i;
    }

    // Flush trailing non-escape text.
    if text_start < bytes.len() {
        out.push_str(&input[text_start..]);
    }
    out
}

/// Rewrite an SGR parameter list, scaling every `38;2;R;G;B` and
/// `48;2;R;G;B` subsequence's RGB triple by [`DIM_SCALE_PERCENT`].
///
/// Returns the full SGR sequence (`\x1b[<params>m`). If parsing
/// fails for any reason, returns the original sequence unchanged
/// (defensive — only known-shape parameter lists are rewritten).
fn rewrite_sgr(params: &[u8]) -> String {
    let s = match std::str::from_utf8(params) {
        Ok(s) => s,
        Err(_) => return passthrough_sgr(params),
    };
    let parsed: Result<Vec<u32>, _> = if s.is_empty() {
        Ok(Vec::new())
    } else {
        s.split(';').map(str::parse::<u32>).collect()
    };
    let mut values = match parsed {
        Ok(v) => v,
        Err(_) => return passthrough_sgr(params),
    };

    // Walk for `[38, 2, R, G, B]` and `[48, 2, R, G, B]` subsequences.
    // Any matched subsequence advances `idx` by 5; otherwise advance
    // by 1 so we don't miss a pattern that starts mid-list (e.g.
    // `[1, 38, 2, R, G, B]`).
    let mut idx = 0usize;
    while idx + 4 < values.len() {
        if (values[idx] == 38 || values[idx] == 48)
            && values[idx + 1] == 2
            && values[idx + 2] <= 255
            && values[idx + 3] <= 255
            && values[idx + 4] <= 255
        {
            values[idx + 2] = u32::from(scale_channel(values[idx + 2] as u8));
            values[idx + 3] = u32::from(scale_channel(values[idx + 3] as u8));
            values[idx + 4] = u32::from(scale_channel(values[idx + 4] as u8));
            idx += 5;
        } else {
            idx += 1;
        }
    }

    let new_params = values
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(";");
    format!("\x1b[{new_params}m")
}

/// Reconstruct an SGR sequence from its raw parameter bytes without
/// modification. Used as a defensive fallback when parameter parsing
/// fails.
fn passthrough_sgr(params: &[u8]) -> String {
    // Parameters in a well-formed CSI are ASCII; if the caller hands
    // us a non-ASCII slice the lossy conversion is acceptable —
    // we're already on the defensive fallback path.
    let s = std::str::from_utf8(params).unwrap_or("");
    format!("\x1b[{s}m")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dim(c: u8) -> u8 {
        scale_channel(c)
    }

    #[test]
    fn scales_truecolor_foreground() {
        let input = "\x1b[38;2;200;100;50m";
        let expected = format!("\x1b[38;2;{};{};{}m", dim(200), dim(100), dim(50));
        assert_eq!(dim_truecolor(input), expected);
    }

    #[test]
    fn scales_truecolor_background() {
        let input = "\x1b[48;2;255;255;255m";
        let expected = format!("\x1b[48;2;{};{};{}m", dim(255), dim(255), dim(255));
        assert_eq!(dim_truecolor(input), expected);
    }

    #[test]
    fn leaves_cursor_move_unchanged() {
        let input = "\x1b[10;5H";
        assert_eq!(dim_truecolor(input), input);
    }

    #[test]
    fn leaves_clear_screen_unchanged() {
        let input = "\x1b[2J";
        assert_eq!(dim_truecolor(input), input);
    }

    #[test]
    fn leaves_bare_reset_unchanged() {
        // `\x1b[0m` parses as SGR with one parameter `0`. No 38/48
        // subsequence, so the rewrite is a no-op (output is
        // re-serialised but byte-identical).
        let input = "\x1b[0m";
        assert_eq!(dim_truecolor(input), input);
    }

    #[test]
    fn leaves_bold_and_default_bg_unchanged() {
        assert_eq!(dim_truecolor("\x1b[1m"), "\x1b[1m");
        assert_eq!(dim_truecolor("\x1b[49m"), "\x1b[49m");
    }

    #[test]
    fn empty_input_passes_through() {
        assert_eq!(dim_truecolor(""), "");
    }

    #[test]
    fn no_escape_text_passes_through_verbatim() {
        let input = "hello world — ┌─┐ unicode";
        assert_eq!(dim_truecolor(input), input);
    }

    #[test]
    fn back_to_back_escapes_with_no_text_between() {
        let input = "\x1b[38;2;100;100;100m\x1b[0m";
        let expected = format!("\x1b[38;2;{};{};{}m\x1b[0m", dim(100), dim(100), dim(100),);
        assert_eq!(dim_truecolor(input), expected);
    }

    #[test]
    fn chained_sgr_scales_both_fg_and_bg_in_one_pass() {
        // A single SGR with both fg and bg — both triples must be
        // scaled in one rewrite.
        let input = "\x1b[38;2;100;100;100;48;2;200;200;200m";
        let expected = format!(
            "\x1b[38;2;{};{};{};48;2;{};{};{}m",
            dim(100),
            dim(100),
            dim(100),
            dim(200),
            dim(200),
            dim(200),
        );
        assert_eq!(dim_truecolor(input), expected);
    }

    #[test]
    fn text_around_escape_is_preserved_with_utf8() {
        // Box-drawing characters are 3-byte UTF-8 sequences; the
        // text-flush path must not split them.
        let input = "┌\x1b[38;2;100;100;100m─\x1b[0m┐";
        let expected = format!(
            "┌\x1b[38;2;{};{};{}m─\x1b[0m┐",
            dim(100),
            dim(100),
            dim(100),
        );
        assert_eq!(dim_truecolor(input), expected);
    }

    #[test]
    fn scale_channel_known_values() {
        assert_eq!(scale_channel(0), 0);
        assert_eq!(scale_channel(100), 35);
        // 255 * 35 / 100 = 8925 / 100 = 89.
        assert_eq!(scale_channel(255), 89);
    }

    #[test]
    fn malformed_csi_does_not_panic() {
        // Truncated CSI at end of input: the ESC is emitted and the
        // truncated bytes pass through.
        let input = "text\x1b[38;2;1";
        let out = dim_truecolor(input);
        // Must contain the original text and not panic. The exact
        // shape of the trailing bytes is the defensive fallback.
        assert!(out.starts_with("text"));
    }

    #[test]
    fn empty_sgr_passes_through() {
        // `\x1b[m` is shorthand for reset. No params, no subsequence
        // to scale; survives the rewrite as-is.
        let input = "\x1b[m";
        assert_eq!(dim_truecolor(input), input);
    }

    #[test]
    fn indexed_color_passes_through() {
        // 256-color indexed (`38;5;<n>`) is not truecolor; must not
        // be rewritten.
        let input = "\x1b[38;5;200m";
        assert_eq!(dim_truecolor(input), input);
    }

    #[test]
    fn dim_bg_escape_matches_dim_truecolor_for_main_bg() {
        // The "blends with dim" contract: emitting `dim_bg_escape(rgb)`
        // produces the same RGB triple the dim transform would
        // produce when rewriting a freshly-painted `48;2;R;G;B`
        // cell. This invariant is what lets overlays paint cells
        // whose bg is visually indistinguishable from the dimmed
        // surround.
        let rgb = [200u8, 100, 50];
        let raw_bg = format!("\x1b[48;2;{};{};{}m", rgb[0], rgb[1], rgb[2]);
        assert_eq!(dim_bg_escape(rgb), dim_truecolor(&raw_bg));
    }

    #[test]
    fn dim_bg_escape_uses_scaled_channels() {
        let escape = dim_bg_escape([100, 200, 255]);
        let expected = format!(
            "\x1b[48;2;{};{};{}m",
            scale_channel(100),
            scale_channel(200),
            scale_channel(255),
        );
        assert_eq!(escape, expected);
    }
}
