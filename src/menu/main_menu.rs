use crate::draw::box_drawing;

/// Draw the main menu centered on screen.
pub fn draw(term_width: usize, term_height: usize) -> String {
    let w = 40.min(term_width);
    let h = 12.min(term_height);
    let x = (term_width.saturating_sub(w)) / 2;
    let y = (term_height.saturating_sub(h)) / 2;

    let mut out =
        box_drawing::create_box(x, y, w, h, "", true, "rtop menu", "", 0, true);

    let banner_y = y + 2;
    out.push_str(&crate::banner::generate(banner_y, x + 2));

    let opts_y = y + h - 3;
    let opts = "[o] Options  [h] Help  [q] Quit";
    let opts_trunc = crate::tools::uresize(opts, w.saturating_sub(4), false);
    out.push_str(&format!("\x1b[{};{}H{}", opts_y, x + 3, opts_trunc));

    out
}
