//! Post-process pass that fades every truecolor SGR foreground escape
//! in an ANSI buffer toward the theme's main background color, leaving
//! background escapes untouched. Used by the central renderer to fade
//! the widget underlay layer beneath an active modal so the modal
//! clearly has focus.
//!
//! The transform is a single forward byte scan that parses CSI
//! sequences per ECMA-48 §5.4 and rewrites only `38;2;R;G;B`
//! (truecolor foreground) subsequences within SGR (`m`) sequences.
//! Background subsequences (`48;2;R;G;B`) pass through unchanged.
//! Every other escape (cursor positioning, clear-screen,
//! indexed/256-color, bare reset, bold, etc.) and every non-escape
//! byte passes through unchanged.
//!
//! ANSI's SGR 2 ("Dim/Faint") attribute is not used: it is unreliably
//! supported on truecolor escapes (which is what every cell in rtop
//! emits), and it is forward-only — emitting it does not retroactively
//! dim text already on screen. Re-rendering with blended RGB triples
//! is the only reliable approach.
//!
//! ## Why fg-only (not fg + bg)
//!
//! Dimming the background produces two structural problems in a
//! terminal app:
//!
//! * The terminal owns a perimeter of cells around the rendered
//!   region that rtop cannot paint. When rtop dims its own cells'
//!   backgrounds, the terminal perimeter stays at the original color,
//!   producing a visible un-dimmed frame around the dimmed region.
//!   For users whose terminal and rtop share a colour theme this is
//!   especially jarring.
//! * Cells with non-`main_bg` backgrounds (graph bars, selected rows,
//!   meters) get their bg darkened too, which produces colour drift
//!   on widget-specific surfaces rather than a uniform fade signal.
//!
//! Blending the foreground toward the main background, with the
//! background untouched, sidesteps both problems: the rendered
//! region stays at original-theme bg (so it matches the terminal
//! perimeter), and the fade signal travels uniformly through the
//! foreground regardless of which bg a given cell happens to sit on.

/// Opacity (in percent) of the foreground after the dim pass — i.e.
/// the share of the original foreground colour that survives, with
/// the remainder filled in from the main background colour.
///
/// 25% is dim enough to clearly de-emphasise the underlay layer
/// without losing structural detail. Lower values (≤15%) start to
/// collapse low-contrast widget text into the background; higher
/// values (≥50%) leave the underlay competing with the modal for
/// attention.
pub const DIM_FG_OPACITY_PERCENT: u8 = 25;

const ESC: u8 = 0x1B;
const CSI_INTRO: u8 = b'[';

/// Linearly interpolate one 0..=255 channel between `fg` and `bg`
/// using `fg_opacity_pct` as the foreground's share of the blend.
///
/// `fg_opacity_pct = 100` returns `fg` unchanged; `0` returns `bg`;
/// values in between produce a proportional mix. Integer math:
/// `(fg * pct + bg * (100 - pct)) / 100`. The intermediate
/// `fg as u16 * 100 + bg as u16 * 100` peaks at `255 * 100 = 25_500`,
/// well within `u16`, so no overflow.
#[inline]
fn lerp_channel(fg: u8, bg: u8, fg_opacity_pct: u8) -> u8 {
    let fg = fg as u16;
    let bg = bg as u16;
    let pct = fg_opacity_pct as u16;
    ((fg * pct + bg * (100 - pct)) / 100) as u8
}

/// Walk `input` and return a copy with every truecolor SGR foreground
/// escape's RGB triple blended toward `main_bg` by
/// [`DIM_FG_OPACITY_PERCENT`]. Truecolor background escapes and every
/// other byte pass through unchanged.
pub fn dim_truecolor(input: &str, main_bg: [u8; 3]) -> String {
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
            out.push_str(&rewrite_sgr(params, main_bg));
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

/// Rewrite an SGR parameter list, blending every `38;2;R;G;B`
/// (truecolor foreground) subsequence's RGB triple toward `main_bg`
/// by [`DIM_FG_OPACITY_PERCENT`]. Truecolor background subsequences
/// (`48;2;R;G;B`) are not modified.
///
/// Returns the full SGR sequence (`\x1b[<params>m`). If parsing
/// fails for any reason, returns the original sequence unchanged
/// (defensive — only known-shape parameter lists are rewritten).
fn rewrite_sgr(params: &[u8], main_bg: [u8; 3]) -> String {
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

    // Walk for `[38, 2, R, G, B]` subsequences. Any matched
    // subsequence advances `idx` by 5; otherwise advance by 1 so we
    // don't miss a pattern that starts mid-list (e.g.
    // `[1, 38, 2, R, G, B]`). Truecolor background subsequences
    // (`48, 2, R, G, B`) are not rewritten — see the module docs.
    let mut idx = 0usize;
    while idx + 4 < values.len() {
        if values[idx] == 38
            && values[idx + 1] == 2
            && values[idx + 2] <= 255
            && values[idx + 3] <= 255
            && values[idx + 4] <= 255
        {
            values[idx + 2] = u32::from(lerp_channel(
                values[idx + 2] as u8,
                main_bg[0],
                DIM_FG_OPACITY_PERCENT,
            ));
            values[idx + 3] = u32::from(lerp_channel(
                values[idx + 3] as u8,
                main_bg[1],
                DIM_FG_OPACITY_PERCENT,
            ));
            values[idx + 4] = u32::from(lerp_channel(
                values[idx + 4] as u8,
                main_bg[2],
                DIM_FG_OPACITY_PERCENT,
            ));
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

    /// Reference theme bg used throughout the tests. Picked off the
    /// default theme so the formula expectations match a realistic
    /// rtop palette.
    const BG: [u8; 3] = [0x00, 0x00, 0x00];

    /// Shorthand: apply the dim formula to one channel against `BG`.
    fn fade(fg: u8, bg: u8) -> u8 {
        lerp_channel(fg, bg, DIM_FG_OPACITY_PERCENT)
    }

    #[test]
    fn blends_truecolor_foreground_toward_bg() {
        let input = "\x1b[38;2;200;100;50m";
        let bg = [10u8, 20, 30];
        let expected = format!(
            "\x1b[38;2;{};{};{}m",
            fade(200, bg[0]),
            fade(100, bg[1]),
            fade(50, bg[2]),
        );
        assert_eq!(dim_truecolor(input, bg), expected);
    }

    #[test]
    fn leaves_truecolor_background_unchanged() {
        // Background escapes pass through untouched — see the
        // module docs for why the dim transform is fg-only.
        let input = "\x1b[48;2;255;255;255m";
        assert_eq!(dim_truecolor(input, BG), input);
    }

    #[test]
    fn leaves_cursor_move_unchanged() {
        let input = "\x1b[10;5H";
        assert_eq!(dim_truecolor(input, BG), input);
    }

    #[test]
    fn leaves_clear_screen_unchanged() {
        let input = "\x1b[2J";
        assert_eq!(dim_truecolor(input, BG), input);
    }

    #[test]
    fn leaves_bare_reset_unchanged() {
        // `\x1b[0m` parses as SGR with one parameter `0`. No 38/48
        // subsequence, so the rewrite is a no-op (output is
        // re-serialised but byte-identical).
        let input = "\x1b[0m";
        assert_eq!(dim_truecolor(input, BG), input);
    }

    #[test]
    fn leaves_bold_and_default_bg_unchanged() {
        assert_eq!(dim_truecolor("\x1b[1m", BG), "\x1b[1m");
        assert_eq!(dim_truecolor("\x1b[49m", BG), "\x1b[49m");
    }

    #[test]
    fn empty_input_passes_through() {
        assert_eq!(dim_truecolor("", BG), "");
    }

    #[test]
    fn no_escape_text_passes_through_verbatim() {
        let input = "hello world — ┌─┐ unicode";
        assert_eq!(dim_truecolor(input, BG), input);
    }

    #[test]
    fn back_to_back_escapes_with_no_text_between() {
        let input = "\x1b[38;2;100;100;100m\x1b[0m";
        let expected = format!(
            "\x1b[38;2;{};{};{}m\x1b[0m",
            fade(100, BG[0]),
            fade(100, BG[1]),
            fade(100, BG[2]),
        );
        assert_eq!(dim_truecolor(input, BG), expected);
    }

    #[test]
    fn chained_sgr_blends_fg_and_leaves_bg_in_one_pass() {
        // A single SGR with both fg and bg — fg gets blended,
        // bg passes through unchanged.
        let input = "\x1b[38;2;100;100;100;48;2;200;200;200m";
        let bg = [40u8, 50, 60];
        let expected = format!(
            "\x1b[38;2;{};{};{};48;2;200;200;200m",
            fade(100, bg[0]),
            fade(100, bg[1]),
            fade(100, bg[2]),
        );
        assert_eq!(dim_truecolor(input, bg), expected);
    }

    #[test]
    fn text_around_escape_is_preserved_with_utf8() {
        // Box-drawing characters are 3-byte UTF-8 sequences; the
        // text-flush path must not split them.
        let input = "┌\x1b[38;2;100;100;100m─\x1b[0m┐";
        let expected = format!(
            "┌\x1b[38;2;{};{};{}m─\x1b[0m┐",
            fade(100, BG[0]),
            fade(100, BG[1]),
            fade(100, BG[2]),
        );
        assert_eq!(dim_truecolor(input, BG), expected);
    }

    #[test]
    fn lerp_channel_known_values() {
        // Endpoints.
        assert_eq!(lerp_channel(0, 0, 35), 0);
        assert_eq!(lerp_channel(255, 255, 35), 255);
        // Identity at the boundaries.
        assert_eq!(lerp_channel(123, 200, 100), 123);
        assert_eq!(lerp_channel(123, 200, 0), 200);
        // (200 * 35 + 0 * 65) / 100 = 70.
        assert_eq!(lerp_channel(200, 0, 35), 70);
        // (0 * 35 + 200 * 65) / 100 = 130.
        assert_eq!(lerp_channel(0, 200, 35), 130);
        // (100 * 35 + 100 * 65) / 100 = 100 — fg equals bg, blend is a no-op.
        assert_eq!(lerp_channel(100, 100, 35), 100);
    }

    #[test]
    fn dim_pulls_fg_toward_bg_not_toward_black() {
        // Regression guard against the previous "scale toward black"
        // formula. With a light bg and a dark fg, the new transform
        // must move fg *toward* bg (lifting it), not *toward* 0
        // (darkening it).
        let bg = [240u8, 240, 240];
        let input = "\x1b[38;2;20;20;20m";
        let result = dim_truecolor(input, bg);
        // Parse out the R channel — must be greater than the input
        // (20), meaning fg moved toward the lighter bg.
        let r = result
            .strip_prefix("\x1b[38;2;")
            .and_then(|s| s.split(';').next())
            .and_then(|s| s.parse::<u8>().ok())
            .expect("dim output must be a well-formed truecolor fg escape");
        assert!(
            r > 20,
            "fg channel should move toward the lighter bg (got {r}, original 20)"
        );
    }

    #[test]
    fn malformed_csi_does_not_panic() {
        // Truncated CSI at end of input: the ESC is emitted and the
        // truncated bytes pass through.
        let input = "text\x1b[38;2;1";
        let out = dim_truecolor(input, BG);
        // Must contain the original text and not panic. The exact
        // shape of the trailing bytes is the defensive fallback.
        assert!(out.starts_with("text"));
    }

    #[test]
    fn empty_sgr_passes_through() {
        // `\x1b[m` is shorthand for reset. No params, no subsequence
        // to rewrite; survives as-is.
        let input = "\x1b[m";
        assert_eq!(dim_truecolor(input, BG), input);
    }

    #[test]
    fn indexed_color_passes_through() {
        // 256-color indexed (`38;5;<n>`) is not truecolor; must not
        // be rewritten.
        let input = "\x1b[38;5;200m";
        assert_eq!(dim_truecolor(input, BG), input);
    }
}
