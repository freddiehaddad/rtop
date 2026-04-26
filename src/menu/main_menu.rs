use crate::term;

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

/// Colors for the three menu rows: selected uses warm tones, normal uses grays
const COLORS_SELECTED: [&str; 3] = [
    "\x1b[38;2;230;37;37m", // #E62525
    "\x1b[38;2;179;29;29m", // #B31D1D
    "\x1b[38;2;128;20;20m", // #801414
];
const COLORS_NORMAL: [&str; 3] = [
    "\x1b[38;2;204;204;204m", // #CC
    "\x1b[38;2;170;170;170m", // #AA
    "\x1b[38;2;128;128;128m", // #80
];

/// Draw the main menu with a specific item selected (0=Options, 1=Help, 2=Quit).
pub fn draw_with_selection(term_width: usize, term_height: usize, selected: usize) -> String {
    let mut out = String::new();

    // Position: banner centered at y = height/2 - 10
    let banner_y = term_height / 2;
    let banner_y = if banner_y > 10 { banner_y - 10 } else { 1 };

    // Draw banner
    out.push_str(&crate::banner::generate(
        banner_y,
        (term_width.saturating_sub(35)) / 2,
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
            &COLORS_SELECTED
        } else {
            &COLORS_NORMAL
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
