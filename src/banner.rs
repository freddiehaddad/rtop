use crate::term;
use crate::theme::Theme;
use crate::theme_keys as tc;

/// ASCII art banner lines for the rtop splash/menu screen.
pub const BANNER: &[&str] = &[
    "██████╗ ████████╗ ██████╗ ██████╗ ",
    "██╔══██╗╚══██╔══╝██╔═══██╗██╔══██╗",
    "██████╔╝   ██║   ██║   ██║██████╔╝",
    "██╔══██╗   ██║   ██║   ██║██╔═══╝ ",
    "██║  ██║   ██║   ╚██████╔╝██║     ",
    "╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝     ",
];

/// Build a 6-step brightness gradient from an RGB color.
fn gradient6(rgb: [u8; 3]) -> [String; 6] {
    let scale = |c: u8, pct: u32| -> u8 { (c as u32 * pct / 100).min(255) as u8 };
    let pcts = [100u32, 85, 72, 60, 50, 40];
    std::array::from_fn(|i| {
        let p = pcts[i];
        format!(
            "\x1b[38;2;{};{};{}m",
            scale(rgb[0], p),
            scale(rgb[1], p),
            scale(rgb[2], p)
        )
    })
}

/// Build a 3-step brightness gradient from an RGB color.
pub fn gradient3(rgb: [u8; 3]) -> [String; 3] {
    let scale = |c: u8, pct: u32| -> u8 { (c as u32 * pct / 100).min(255) as u8 };
    let pcts = [100u32, 70, 45];
    std::array::from_fn(|i| {
        let p = pcts[i];
        format!(
            "\x1b[38;2;{};{};{}m",
            scale(rgb[0], p),
            scale(rgb[1], p),
            scale(rgb[2], p)
        )
    })
}

/// Paint `text` at terminal column `x`, row `y`, in `color`, treating
/// space characters as transparent: the cursor advances over them
/// without emitting bytes, preserving whatever content already
/// occupies those cells.
///
/// Used by free-floating modals (the main-menu banner and item rows)
/// so the negative space inside each glyph row shows the underlying
/// — typically dimmed — widget content instead of overwriting it
/// with a flat background rectangle.
///
/// Contiguous non-space runs are emitted as a single substring
/// prefixed by one cursor-move and one `color` escape. Runs of
/// spaces produce no output at all; the cursor's logical column
/// advances via the next run's move. All characters used by the
/// banner and menu glyphs are unicode width 1, so byte-to-column
/// mapping is straightforward.
pub(crate) fn paint_transparent_row(buf: &mut String, x: usize, y: usize, color: &str, text: &str) {
    let mut run_start: Option<usize> = None;
    let mut run_text = String::new();

    for (col, c) in text.chars().enumerate() {
        if c == ' ' {
            if let Some(start) = run_start.take() {
                buf.push_str(&term::mv(x + start, y));
                buf.push_str(color);
                buf.push_str(&run_text);
                run_text.clear();
            }
        } else {
            if run_start.is_none() {
                run_start = Some(col);
            }
            run_text.push(c);
        }
    }

    if let Some(start) = run_start {
        buf.push_str(&term::mv(x + start, y));
        buf.push_str(color);
        buf.push_str(&run_text);
    }
}

/// Render the banner positioned at (x, y) with a vertical gradient derived from the theme.
pub fn generate(y: usize, x: usize, theme: &Theme) -> String {
    let rgb = theme.rgb(tc::MENU_HI_FG);
    let gradient = gradient6(rgb);
    let mut out = String::new();
    for (i, line) in BANNER.iter().enumerate() {
        paint_transparent_row(&mut out, x + 1, y + i + 1, &gradient[i], line);
    }
    out.push_str(term::RESET);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_has_six_lines() {
        assert_eq!(BANNER.len(), 6);
    }

    #[test]
    fn generate_positions_correctly() {
        let theme = Theme::new();
        let out = generate(0, 0, &theme);
        assert!(out.contains("\x1b[1;1H"));
        assert!(out.contains("██████╗"));
    }

    #[test]
    fn gradient3_produces_three_colors() {
        let g = gradient3([200, 100, 50]);
        assert_eq!(g.len(), 3);
        assert!(g[0].contains("200;100;50"));
    }

    // --- paint_transparent_row ---

    #[test]
    fn transparent_row_emits_non_space_runs_only() {
        let color = "\x1b[38;2;1;2;3m";
        let mut out = String::new();
        paint_transparent_row(&mut out, 5, 10, color, "ab cd");

        // Two runs: "ab" at col 5, "cd" at col 8 (5 + 3).
        assert_eq!(out, format!("\x1b[10;5H{color}ab\x1b[10;8H{color}cd"));
    }

    #[test]
    fn transparent_row_emits_nothing_for_empty_text() {
        let mut out = String::new();
        paint_transparent_row(&mut out, 1, 1, "\x1b[0m", "");
        assert!(out.is_empty());
    }

    #[test]
    fn transparent_row_emits_nothing_for_all_space_text() {
        // A row of pure spaces produces no output — the cursor never
        // moves, no color is emitted, no character is written. The
        // underlying cells are preserved entirely.
        let mut out = String::new();
        paint_transparent_row(&mut out, 1, 1, "\x1b[0m", "    ");
        assert!(out.is_empty());
    }

    #[test]
    fn transparent_row_skips_leading_and_trailing_spaces() {
        let color = "\x1b[38;2;0;0;0m";
        let mut out = String::new();
        paint_transparent_row(&mut out, 2, 3, color, "  glyph  ");

        // Single run "glyph" starting at col 2 + 2 = 4.
        assert_eq!(out, format!("\x1b[3;4H{color}glyph"));
    }

    #[test]
    fn transparent_row_never_emits_space_glyphs() {
        // The whole point of the transparent treatment: no space
        // character is ever written, so no cell along a "space"
        // column is overwritten with the current SGR background.
        let mut out = String::new();
        paint_transparent_row(&mut out, 1, 1, "\x1b[0m", "a b c d");
        assert!(
            !out.contains(' '),
            "transparent row must not emit any space character; got {out:?}"
        );
    }

    #[test]
    fn transparent_row_handles_multibyte_glyphs() {
        // Box-drawing chars are multi-byte UTF-8 but unicode width
        // 1; column accounting is by `chars()`, not bytes.
        let color = "\x1b[38;2;0;0;0m";
        let mut out = String::new();
        paint_transparent_row(&mut out, 1, 1, color, "┌─┐ │ │ └─┘");

        // Three runs: "┌─┐" at col 1, "│" at col 5, "│" at col 7,
        // "└─┘" at col 9. Verify each run's column placement.
        assert!(out.contains(&format!("\x1b[1;1H{color}┌─┐")));
        assert!(out.contains(&format!("\x1b[1;5H{color}│")));
        assert!(out.contains(&format!("\x1b[1;7H{color}│")));
        assert!(out.contains(&format!("\x1b[1;9H{color}└─┘")));
    }
}
