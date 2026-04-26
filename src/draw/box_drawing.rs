/// Box-drawing characters.
pub mod symbols {
    pub const H_LINE: &str = "─";
    pub const V_LINE: &str = "│";
    pub const DOTTED_V_LINE: &str = "╎";
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
    pub const METER_CHAR: &str = "■";
}

/// Create a box frame with optional title, fill, and line color.
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

    // Move to position
    out.push_str(&format!("\x1b[{};{}H", y + 1, x + 1));
    out.push_str(line_color);

    // Top border
    out.push_str(tl);
    if !title.is_empty() {
        let title_str = format!(" {} ", title);
        let remaining = width.saturating_sub(2 + title_str.len());
        let left_dash = remaining / 2;
        let right_dash = remaining - left_dash;
        out.push_str(&symbols::H_LINE.repeat(left_dash));
        out.push_str(&title_str);
        out.push_str(&symbols::H_LINE.repeat(right_dash));
    } else {
        out.push_str(&symbols::H_LINE.repeat(width - 2));
    }
    // Add box number as superscript
    if num > 0 && (num as usize) < symbols::SUPERSCRIPT.len() {
        // Replace last h_line with superscript
        out.push_str(symbols::SUPERSCRIPT[num as usize]);
    }
    out.push_str(tr);

    // Middle rows
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

    // Bottom border
    out.push_str(&format!("\x1b[{};{}H", y + height, x + 1));
    out.push_str(bl);
    if !title2.is_empty() {
        let title_str = format!(" {} ", title2);
        let remaining = width.saturating_sub(2 + title_str.len());
        let left_dash = remaining / 2;
        let right_dash = remaining - left_dash;
        out.push_str(&symbols::H_LINE.repeat(left_dash));
        out.push_str(&title_str);
        out.push_str(&symbols::H_LINE.repeat(right_dash));
    } else {
        out.push_str(&symbols::H_LINE.repeat(width - 2));
    }
    out.push_str(br);

    out.push_str("\x1b[0m"); // Reset
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
        let b = create_box(0, 0, 10, 3, "", false, "", "", 1, false);
        assert!(b.contains("¹"));
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
