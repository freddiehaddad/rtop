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

/// Render the banner positioned at (x, y) using ANSI cursor movement.
pub fn generate(y: usize, x: usize) -> String {
    let mut out = String::new();
    for (i, line) in BANNER.iter().enumerate() {
        out.push_str(&format!("{}{}", term::mv(x + 1, y + i + 1), line));
    }
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
