use crate::domain::network::NetInfo;
use crate::draw::box_drawing;
use crate::theme::Theme;
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
    theme: &Theme,
) -> String {
    let box_color = theme.c("net_box");
    let fg = theme.c("main_fg");
    let dl_grad = theme.g("download");
    let ul_grad = theme.g("upload");
    let hi = theme.c("hi_fg");

    let mut out = box_drawing::create_box(x, y, width, height, box_color, false, "net", "", 0, rounded);

    if let Some(dl) = net.stat.get("download") {
        let speed = tools::floating_humanizer(dl.speed, true, 0, false, true, false);
        let total = tools::floating_humanizer(dl.total, true, 0, false, false, false);
        let dl_color = if !dl_grad.is_empty() {
            let idx = if dl.top > 0 { (dl.speed * 100 / dl.top.max(1)) as usize } else { 0 };
            &dl_grad[idx.min(100)]
        } else { fg };
        let line = format!("{}▼ {}{} {}Total: {}", dl_color, speed, fg, fg, total);
        out.push_str(&format!("\x1b[{};{}H{}", y + 2, x + 2, line));
    }

    if let Some(ul) = net.stat.get("upload") {
        let speed = tools::floating_humanizer(ul.speed, true, 0, false, true, false);
        let total = tools::floating_humanizer(ul.total, true, 0, false, false, false);
        let ul_color = if !ul_grad.is_empty() {
            let idx = if ul.top > 0 { (ul.speed * 100 / ul.top.max(1)) as usize } else { 0 };
            &ul_grad[idx.min(100)]
        } else { fg };
        let line = format!("{}▲ {}{} {}Total: {}", ul_color, speed, fg, fg, total);
        out.push_str(&format!("\x1b[{};{}H{}", y + 3, x + 2, line));
    }

    // IP address
    if height > 5 && !net.ipv4.is_empty() {
        out.push_str(&format!("\x1b[{};{}H{}IPv4: {}", y + 4, x + 2, fg, net.ipv4));
    }

    // Interface name at bottom
    let iface_line = format!("{}< {}{}{} >", fg, hi, iface, fg);
    out.push_str(&format!("\x1b[{};{}H{}", y + height - 1, x + 2, iface_line));

    out.push_str("\x1b[0m");
    out
}

