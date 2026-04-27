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

/// Render the banner positioned at (x, y) with a vertical gradient derived from the theme.
pub fn generate(y: usize, x: usize, theme: &Theme) -> String {
    let rgb = theme.rgbs.get(tc::HI_FG).copied().unwrap_or_default();
    let gradient = gradient6(rgb);
    let mut out = String::new();
    for (i, line) in BANNER.iter().enumerate() {
        out.push_str(&format!(
            "{}{}{}",
            term::mv(x + 1, y + i + 1),
            gradient[i],
            line
        ));
    }
    out.push_str("\x1b[0m");
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
}
