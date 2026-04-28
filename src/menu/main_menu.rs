use crate::banner;
use crate::term;
use crate::theme::Theme;
use crate::theme_keys as tc;

/// Menu item ASCII art: normal (thin lines) and selected (thick lines)
const MENU_NORMAL: [&[&str]; 3] = [
    &[
        "┌─┐┌─┐┌┬┐┬┌─┐┌┐┌┌─┐",
        "│ │├─┘ │ ││ ││││└─┐",
        "└─┘┴   ┴ ┴└─┘┘└┘└─┘",
    ],
    &["┬ ┬┌─┐┬  ┌─┐", "├─┤├┤ │  ├─┘", "┴ ┴└─┘┴─┘┴  "],
    &["┌─┐ ┬ ┬ ┬┌┬┐", "│─┼┐│ │ │ │ ", "└─┘└└─┘ ┴ ┴ "],
];

const MENU_SELECTED: [&[&str]; 3] = [
    &[
        "╔═╗╔═╗╔╦╗╦╔═╗╔╗╔╔═╗",
        "║ ║╠═╝ ║ ║║ ║║║║╚═╗",
        "╚═╝╩   ╩ ╩╚═╝╝╚╝╚═╝",
    ],
    &["╦ ╦╔═╗╦  ╔═╗", "╠═╣╠╣ ║  ╠═╝", "╩ ╩╚═╝╩═╝╩  "],
    &["╔═╗ ╦ ╦ ╦╔╦╗ ", "║═╬╗║ ║ ║ ║  ", "╚═╝╚╚═╝ ╩ ╩  "],
];

const MENU_WIDTHS: [usize; 3] = [19, 12, 12];

/// Draw the main menu with a specific item selected (0=Options, 1=Help, 2=Quit).
/// Colors are derived from the theme's hi_fg (selected) and main_fg (normal).
pub fn draw_with_selection(
    term_width: usize,
    term_height: usize,
    selected: usize,
    theme: &Theme,
) -> String {
    let hi_rgb = theme.rgb(tc::HI_FG);
    let fg_rgb = theme.rgb(tc::MAIN_FG);
    let colors_selected = banner::gradient3(hi_rgb);
    let colors_normal = banner::gradient3(fg_rgb);

    let mut out = String::new();

    // Position: banner centered at y = height/2 - 10
    let banner_y = term_height / 2;
    let banner_y = if banner_y > 10 { banner_y - 10 } else { 1 };

    // Draw banner
    out.push_str(&banner::generate(
        banner_y,
        (term_width.saturating_sub(35)) / 2,
        theme,
    ));

    // Menu items start below the banner (6 lines of banner + 1 gap)
    let mut cy = banner_y + 7;

    for i in 0..3 {
        let menu = if i == selected {
            &MENU_SELECTED[i]
        } else {
            &MENU_NORMAL[i]
        };
        let colors = if i == selected {
            &colors_selected
        } else {
            &colors_normal
        };
        let w = MENU_WIDTHS[i];
        let menu_x = (term_width.saturating_sub(w)) / 2;

        for (line_idx, line) in menu.iter().enumerate() {
            out.push_str(&format!(
                "{}{}{}",
                term::mv(menu_x + 1, cy),
                colors[line_idx],
                line,
            ));
            cy += 1;
        }
    }

    out.push_str("\x1b[0m");
    out
}
