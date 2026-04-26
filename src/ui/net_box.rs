use crate::domain::network::NetInfo;
use crate::draw::box_drawing;
use crate::tools;

/// Draw the network box into an ANSI string.
pub fn draw(
    net: &NetInfo,
    iface: &str,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    rounded: bool,
) -> String {
    let mut out = box_drawing::create_box(x, y, width, height, "", false, "net", "", 0, rounded);

    if let Some(dl) = net.stat.get("download") {
        let speed = tools::floating_humanizer(dl.speed, true, 0, false, true, false);
        let total = tools::floating_humanizer(dl.total, true, 0, false, false, false);
        let line = format!("▼ {}  Total: {}", speed, total);
        let line_trunc = tools::uresize(&line, width.saturating_sub(4), false);
        out.push_str(&format!("\x1b[{};{}H{}", y + 2, x + 2, line_trunc));
    }

    if let Some(ul) = net.stat.get("upload") {
        let speed = tools::floating_humanizer(ul.speed, true, 0, false, true, false);
        let total = tools::floating_humanizer(ul.total, true, 0, false, false, false);
        let line = format!("▲ {}  Total: {}", speed, total);
        let line_trunc = tools::uresize(&line, width.saturating_sub(4), false);
        out.push_str(&format!("\x1b[{};{}H{}", y + 3, x + 2, line_trunc));
    }

    // Interface name at bottom
    let iface_line = format!("< {} >", iface);
    let iface_trunc = tools::uresize(&iface_line, width.saturating_sub(4), false);
    out.push_str(&format!(
        "\x1b[{};{}H{}",
        y + height - 1,
        x + 2,
        iface_trunc
    ));

    out
}
