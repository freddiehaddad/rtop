/// Box-drawing characters.
#[allow(dead_code)]
pub mod symbols {
    pub const H_LINE: &str = "─";
    pub const V_LINE: &str = "│";
    pub const LEFT_UP: &str = "┌";
    pub const RIGHT_UP: &str = "┐";
    pub const LEFT_DOWN: &str = "└";
    pub const RIGHT_DOWN: &str = "┘";
    pub const ROUND_LEFT_UP: &str = "╭";
    pub const ROUND_RIGHT_UP: &str = "╮";
    pub const ROUND_LEFT_DOWN: &str = "╰";
    pub const ROUND_RIGHT_DOWN: &str = "╯";
    pub const DIV_RIGHT: &str = "┤";
    pub const DIV_LEFT: &str = "├";
    pub const DIV_UP: &str = "┬";
    pub const DIV_DOWN: &str = "┴";
    pub const UP_ARROW: &str = "↑";
    pub const DOWN_ARROW: &str = "↓";
    pub const LEFT_ARROW: &str = "←";
    pub const RIGHT_ARROW: &str = "→";
    pub const ENTER: &str = "↵";
    pub const SUPERSCRIPT: [&str; 10] = ["⁰", "¹", "²", "³", "⁴", "⁵", "⁶", "⁷", "⁸", "⁹"];
}

/// Box-drawing title inset characters (matching btop).
/// These create a notch in the border line for the title text.
pub mod title_syms {
    pub const TITLE_LEFT: &str = "┐";
    pub const TITLE_RIGHT: &str = "┌";
    pub const TITLE_LEFT_DOWN: &str = "┘";
    pub const TITLE_RIGHT_DOWN: &str = "└";
}

/// Create a box frame matching btop's createBox exactly.
///
/// btop's algorithm:
/// 1. Draw horizontal lines (full width) on top and bottom rows
/// 2. Draw vertical lines (and optional fill) on middle rows
/// 3. Draw corners on top of the horizontal lines
/// 4. Draw title at (y, x+2) using title_left/title_right inset chars
pub fn create_box(
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    line_color: &str,
    fill: bool,
    title: &str,
    title2: &str,
    num: u8,
    rounded: bool,
) -> String {
    if width < 2 || height < 2 {
        return String::new();
    }

    let color = if line_color.is_empty() { "" } else { line_color };

    let (tl, tr, bl, br) = if rounded {
        (
            symbols::ROUND_LEFT_UP,
            symbols::ROUND_RIGHT_UP,
            symbols::ROUND_LEFT_DOWN,
            symbols::ROUND_RIGHT_DOWN,
        )
    } else {
        (
            symbols::LEFT_UP,
            symbols::RIGHT_UP,
            symbols::LEFT_DOWN,
            symbols::RIGHT_DOWN,
        )
    };

    let mut out = String::new();
    out.push_str("\x1b[0m");
    out.push_str(color);

    // Step 1: Draw horizontal lines on top and bottom
    // Top line: full width of h_line at (y+1, x+1)
    out.push_str(&format!(
        "\x1b[{};{}H{}",
        y + 1, x + 1,
        symbols::H_LINE.repeat(width)
    ));
    // Bottom line
    out.push_str(&format!(
        "\x1b[{};{}H{}",
        y + height, x + 1,
        symbols::H_LINE.repeat(width)
    ));

    // Step 2: Draw vertical lines and fill on middle rows
    for row in 1..(height - 1) {
        out.push_str(&format!("\x1b[{};{}H", y + 1 + row, x + 1));
        out.push_str(symbols::V_LINE);
        if fill {
            out.push_str(&" ".repeat(width - 2));
        } else {
            out.push_str(&format!("\x1b[{}C", width - 2));
        }
        out.push_str(symbols::V_LINE);
    }

    // Step 3: Draw corners (overwriting the h_line at the corner positions)
    out.push_str(&format!("\x1b[{};{}H{}", y + 1, x + 1, tl));
    out.push_str(&format!("\x1b[{};{}H{}", y + 1, x + width, tr));
    out.push_str(&format!("\x1b[{};{}H{}", y + height, x + 1, bl));
    out.push_str(&format!("\x1b[{};{}H{}", y + height, x + width, br));

    // Step 4: Draw title at (y, x+2) if defined — matching btop format:
    // title_left + bold + hi_fg_numbering + title_color + title + unbold + line_color + title_right
    if !title.is_empty() {
        let numbering = if num > 0 && (num as usize) < symbols::SUPERSCRIPT.len() {
            symbols::SUPERSCRIPT[num as usize]
        } else {
            ""
        };
        // btop uses: hi_fg for number, title color for text, bold for both
        out.push_str(&format!(
            "\x1b[{};{}H{}\x1b[1m{}{}\x1b[22m{}{}",
            y + 1, x + 3,
            title_syms::TITLE_LEFT,
            numbering,  // number in current color (line_color which is box color)
            title,       // title text
            color,
            title_syms::TITLE_RIGHT,
        ));
    }

    // Title2 on bottom border
    if !title2.is_empty() {
        out.push_str(&format!(
            "\x1b[{};{}H{}{}{}{}",
            y + height, x + 3,
            title_syms::TITLE_LEFT_DOWN,
            title2,
            color,
            title_syms::TITLE_RIGHT_DOWN,
        ));
    }

    out.push_str(&format!("\x1b[0m\x1b[{};{}H", y + 2, x + 2));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_box_minimal() {
        let b = create_box(0, 0, 4, 3, "", false, "", "", 0, false);
        assert!(b.contains(symbols::LEFT_UP));
        assert!(b.contains(symbols::RIGHT_DOWN));
        assert!(b.contains(symbols::H_LINE));
        assert!(b.contains(symbols::V_LINE));
    }

    #[test]
    fn create_box_with_title() {
        let b = create_box(0, 0, 20, 5, "", false, "cpu", "", 0, false);
        assert!(b.contains("cpu"));
    }

    #[test]
    fn create_box_rounded_corners() {
        let b = create_box(0, 0, 10, 5, "", false, "", "", 0, true);
        assert!(b.contains(symbols::ROUND_LEFT_UP));
        assert!(b.contains(symbols::ROUND_RIGHT_DOWN));
    }

    #[test]
    fn create_box_with_number() {
        let b = create_box(0, 0, 20, 3, "", false, "test", "", 1, false);
        assert!(b.contains("¹"));
        assert!(b.contains("test"));
    }

    #[test]
    fn create_box_fill() {
        let b = create_box(0, 0, 6, 4, "", true, "", "", 0, false);
        // Fill should contain spaces between vertical lines
        assert!(b.contains("    ")); // 4 spaces (width-2)
    }

    #[test]
    fn create_box_too_small_returns_empty() {
        assert_eq!(create_box(0, 0, 1, 1, "", false, "", "", 0, false), "");
    }
}
