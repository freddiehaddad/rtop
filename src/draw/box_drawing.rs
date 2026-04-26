use crate::term;

/// Box-drawing characters.
pub mod symbols {
    /// Horizontal line `─`.
    pub const H_LINE: &str = "─";
    /// Vertical line `│`.
    pub const V_LINE: &str = "│";
    /// Top-left corner `┌`.
    pub const LEFT_UP: &str = "┌";
    /// Top-right corner `┐`.
    pub const RIGHT_UP: &str = "┐";
    /// Bottom-left corner `└`.
    pub const LEFT_DOWN: &str = "└";
    /// Bottom-right corner `┘`.
    pub const RIGHT_DOWN: &str = "┘";
    /// Rounded top-left corner `╭`.
    pub const ROUND_LEFT_UP: &str = "╭";
    /// Rounded top-right corner `╮`.
    pub const ROUND_RIGHT_UP: &str = "╮";
    /// Rounded bottom-left corner `╰`.
    pub const ROUND_LEFT_DOWN: &str = "╰";
    /// Rounded bottom-right corner `╯`.
    pub const ROUND_RIGHT_DOWN: &str = "╯";
    /// Right T-junction `┤`.
    pub const DIV_RIGHT: &str = "┤";
    /// Left T-junction `├`.
    pub const DIV_LEFT: &str = "├";
    /// Top T-junction `┬`.
    pub const DIV_UP: &str = "┬";
    /// Bottom T-junction `┴`.
    pub const DIV_DOWN: &str = "┴";
    /// Up arrow `↑`.
    pub const UP_ARROW: &str = "↑";
    /// Down arrow `↓`.
    pub const DOWN_ARROW: &str = "↓";
    /// Left arrow `←`.
    pub const LEFT_ARROW: &str = "←";
    /// Right arrow `→`.
    pub const RIGHT_ARROW: &str = "→";
    /// Enter/return symbol `↵`.
    pub const ENTER: &str = "↵";
    /// Unicode superscript digits 0–9.
    pub const SUPERSCRIPT: [&str; 10] = ["⁰", "¹", "²", "³", "⁴", "⁵", "⁶", "⁷", "⁸", "⁹"];
}

/// Box-drawing title inset characters (matching btop).
/// These create a notch in the border line for the title text.
pub mod title_syms {
    /// Left inset for title on top border `┐`.
    pub const TITLE_LEFT: &str = "┐";
    /// Right inset for title on top border `┌`.
    pub const TITLE_RIGHT: &str = "┌";
    /// Left inset for title on bottom border `┘`.
    pub const TITLE_LEFT_DOWN: &str = "┘";
    /// Right inset for title on bottom border `└`.
    pub const TITLE_RIGHT_DOWN: &str = "└";
}

/// Configuration for drawing a box frame.
pub struct BoxConfig<'a> {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub line_color: &'a str,
    pub fill: bool,
    pub title: &'a str,
    pub title2: &'a str,
    pub num: u8,
    pub rounded: bool,
}

/// Create a box frame matching btop's createBox exactly.
///
/// btop's algorithm:
/// 1. Draw horizontal lines (full width) on top and bottom rows
/// 2. Draw vertical lines (and optional fill) on middle rows
/// 3. Draw corners on top of the horizontal lines
/// 4. Draw title at (y, x+2) using title_left/title_right inset chars
pub fn create_box(cfg: &BoxConfig) -> String {
    let x = cfg.x;
    let y = cfg.y;
    let width = cfg.width;
    let height = cfg.height;
    let fill = cfg.fill;
    let title = cfg.title;
    let title2 = cfg.title2;
    let num = cfg.num;
    let rounded = cfg.rounded;
    if width < 2 || height < 2 {
        return String::new();
    }

    let color = if cfg.line_color.is_empty() { "" } else { cfg.line_color };

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
        "{}{}",
        term::mv(x + 1, y + 1),
        symbols::H_LINE.repeat(width)
    ));
    // Bottom line
    out.push_str(&format!(
        "{}{}",
        term::mv(x + 1, y + height),
        symbols::H_LINE.repeat(width)
    ));

    // Step 2: Draw vertical lines and fill on middle rows
    for row in 1..(height - 1) {
        out.push_str(&term::mv(x + 1, y + 1 + row));
        out.push_str(symbols::V_LINE);
        if fill {
            out.push_str(&" ".repeat(width - 2));
        } else {
            out.push_str(&format!("\x1b[{}C", width - 2));
        }
        out.push_str(symbols::V_LINE);
    }

    // Step 3: Draw corners (overwriting the h_line at the corner positions)
    out.push_str(&format!("{}{}", term::mv(x + 1, y + 1), tl));
    out.push_str(&format!("{}{}", term::mv(x + width, y + 1), tr));
    out.push_str(&format!("{}{}", term::mv(x + 1, y + height), bl));
    out.push_str(&format!("{}{}", term::mv(x + width, y + height), br));

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
            "{}{}\x1b[1m{}{}\x1b[22m{}{}",
            term::mv(x + 3, y + 1),
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
            "{}{}{}{}{}",
            term::mv(x + 3, y + height),
            title_syms::TITLE_LEFT_DOWN,
            title2,
            color,
            title_syms::TITLE_RIGHT_DOWN,
        ));
    }

    out.push_str(&format!("\x1b[0m{}", term::mv(x + 2, y + 2)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(x: usize, y: usize, w: usize, h: usize) -> BoxConfig<'static> {
        BoxConfig { x, y, width: w, height: h, line_color: "", fill: false, title: "", title2: "", num: 0, rounded: false }
    }

    #[test]
    fn create_box_minimal() {
        let b = create_box(&cfg(0, 0, 4, 3));
        assert!(b.contains(symbols::LEFT_UP));
        assert!(b.contains(symbols::RIGHT_DOWN));
        assert!(b.contains(symbols::H_LINE));
        assert!(b.contains(symbols::V_LINE));
    }

    #[test]
    fn create_box_with_title() {
        let b = create_box(&BoxConfig { title: "cpu", ..cfg(0, 0, 20, 5) });
        assert!(b.contains("cpu"));
    }

    #[test]
    fn create_box_rounded_corners() {
        let b = create_box(&BoxConfig { rounded: true, ..cfg(0, 0, 10, 5) });
        assert!(b.contains(symbols::ROUND_LEFT_UP));
        assert!(b.contains(symbols::ROUND_RIGHT_DOWN));
    }

    #[test]
    fn create_box_with_number() {
        let b = create_box(&BoxConfig { title: "test", num: 1, ..cfg(0, 0, 20, 3) });
        assert!(b.contains("¹"));
        assert!(b.contains("test"));
    }

    #[test]
    fn create_box_fill() {
        let b = create_box(&BoxConfig { fill: true, ..cfg(0, 0, 6, 4) });
        // Fill should contain spaces between vertical lines
        assert!(b.contains("    ")); // 4 spaces (width-2)
    }

    #[test]
    fn create_box_too_small_returns_empty() {
        assert_eq!(create_box(&cfg(0, 0, 1, 1)), "");
    }
}
