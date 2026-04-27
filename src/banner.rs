use crate::term;

/// ASCII art banner lines for the rtop splash/menu screen.
pub const BANNER: &[&str] = &[
    "██████╗ ████████╗ ██████╗ ██████╗ ",
    "██╔══██╗╚══██╔══╝██╔═══██╗██╔══██╗",
    "██████╔╝   ██║   ██║   ██║██████╔╝",
    "██╔══██╗   ██║   ██║   ██║██╔═══╝ ",
    "██║  ██║   ██║   ╚██████╔╝██║     ",
    "╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝     ",
];

/// Render the banner positioned at (x, y) with a vertical color gradient.
pub fn generate(y: usize, x: usize) -> String {
    // Warm gradient: bright orange-red at top → deep crimson at bottom
    const GRADIENT: [&str; 6] = [
        "\x1b[38;2;255;100;50m", // bright orange-red
        "\x1b[38;2;240;70;45m",
        "\x1b[38;2;220;50;40m",
        "\x1b[38;2;200;35;35m",
        "\x1b[38;2;175;25;30m",
        "\x1b[38;2;145;20;25m", // deep crimson
    ];
    let mut out = String::new();
    for (i, line) in BANNER.iter().enumerate() {
        let color = GRADIENT[i.min(GRADIENT.len() - 1)];
        out.push_str(&format!("{}{}{}", term::mv(x + 1, y + i + 1), color, line));
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
        let out = generate(0, 0);
        assert!(out.contains("\x1b[1;1H"));
        assert!(out.contains("██████╗"));
    }
}
