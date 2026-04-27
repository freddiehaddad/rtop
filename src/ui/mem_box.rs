use crate::domain::memory::MemInfo;
use crate::draw::box_drawing;
use crate::draw::meter::Meter;
use crate::term;
use crate::theme::Theme;
use crate::tools;

use super::BoxArea;

/// Draw the memory box into an ANSI string.
///
/// Layout:
/// ╭─ mem ──────────────────────╮
/// │ Used  ■■■■■■■■■■■░░░ 5.2G │
/// │ Avail ■■■■░░░░░░░░░░ 3.1G │
/// │ Cache ■■■░░░░░░░░░░░ 1.8G │
/// │ Free  ■■░░░░░░░░░░░░ 0.9G │
/// │                            │
/// │ Swap  ■■░░░░░░░░░░░░ 1.0G │
/// │   1.0G / 8.0G              │
/// ╰────────────────────────────╯
pub fn draw(mem: &MemInfo, area: &BoxArea, theme: &Theme, show_swap: bool) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.c("mem_box");
    let fg = theme.c("main_fg");
    let title_color = theme.c("title");
    let hi = theme.c("hi_fg");
    let used_grad = theme.g("used");
    let free_grad = theme.g("free");
    let cached_grad = theme.g("cached");
    let avail_grad = theme.g("available");

    let inner_h = height.saturating_sub(2);

    let mut out = box_drawing::create_box(&box_drawing::BoxConfig {
        x,
        y,
        width,
        height,
        line_color: box_color,
        fill: true,
        title: "mem",
        title2: "",
        num: 2,
        rounded,
        hi_color: hi,
        title_color,
    });

    let total_bytes =
        mem.stats.get("used").unwrap_or(&0) + mem.stats.get("available").unwrap_or(&0);
    let meter_area = width.saturating_sub(4);
    let label_w = 6;
    let meter_w = meter_area.saturating_sub(label_w + 6).max(5);
    let meter_bg = theme.c("meter_bg");
    let used_meter = Meter::new(meter_w, used_grad, meter_bg);
    let avail_meter = Meter::new(meter_w, avail_grad, meter_bg);
    let cached_meter = Meter::new(meter_w, cached_grad, meter_bg);
    let free_meter = Meter::new(meter_w, free_grad, meter_bg);
    let mut row = 0;

    // Used
    let used = *mem.stats.get("used").unwrap_or(&0);
    let used_pct = (used * 100).checked_div(total_bytes).unwrap_or(0) as i32;
    let used_color = gradient_color(used_grad, used_pct as i64);
    let used_str = tools::floating_humanizer(used, true, 0, false, false, false);
    if row < inner_h {
        out.push_str(&format!(
            "{}{}Used  {}  {}{}",
            term::mv(x + 2, y + 2 + row),
            title_color,
            used_meter.render(used_pct),
            used_color,
            used_str
        ));
        row += 1;
    }

    // Available
    let avail = *mem.stats.get("available").unwrap_or(&0);
    let avail_pct = (avail * 100).checked_div(total_bytes).unwrap_or(0) as i32;
    let avail_color = gradient_color(avail_grad, avail_pct as i64);
    let avail_str = tools::floating_humanizer(avail, true, 0, false, false, false);
    if row < inner_h {
        out.push_str(&format!(
            "{}{}Avail {}  {}{}",
            term::mv(x + 2, y + 2 + row),
            title_color,
            avail_meter.render(avail_pct),
            avail_color,
            avail_str
        ));
        row += 1;
    }

    // Cached
    let cached = *mem.stats.get("cached").unwrap_or(&0);
    if cached > 0 && row < inner_h {
        let cached_pct = (cached * 100).checked_div(total_bytes).unwrap_or(0) as i32;
        let cache_color = gradient_color(cached_grad, cached_pct as i64);
        let cached_str = tools::floating_humanizer(cached, true, 0, false, false, false);
        out.push_str(&format!(
            "{}{}Cache {}  {}{}",
            term::mv(x + 2, y + 2 + row),
            title_color,
            cached_meter.render(cached_pct),
            cache_color,
            cached_str
        ));
        row += 1;
    }

    // Free
    let free = *mem.stats.get("free").unwrap_or(&0);
    let free_pct = (free * 100).checked_div(total_bytes).unwrap_or(0) as i32;
    let free_color = gradient_color(free_grad, free_pct as i64);
    let free_str = tools::floating_humanizer(free, true, 0, false, false, false);
    if row < inner_h {
        out.push_str(&format!(
            "{}{}Free  {}  {}{}",
            term::mv(x + 2, y + 2 + row),
            title_color,
            free_meter.render(free_pct),
            free_color,
            free_str
        ));
        row += 1;
    }

    // Blank line before swap
    if show_swap {
        if row < inner_h {
            row += 1;
        }

        // Swap
        let swap_used = *mem.stats.get("swap_used").unwrap_or(&0);
        let swap_total = *mem.stats.get("swap_total").unwrap_or(&0);
        if swap_total > 0 && row < inner_h {
            let swap_pct = (swap_used * 100 / swap_total.max(1)) as i32;
            let swap_str = tools::floating_humanizer(swap_used, true, 0, false, false, false);
            out.push_str(&format!(
                "{}{}Swap  {}  {}{}",
                term::mv(x + 2, y + 2 + row),
                title_color,
                used_meter.render(swap_pct),
                fg,
                swap_str
            ));
            row += 1;

            // Swap total line
            if row < inner_h {
                let su = tools::floating_humanizer(swap_used, true, 0, false, false, false);
                let st = tools::floating_humanizer(swap_total, true, 0, false, false, false);
                out.push_str(&format!(
                    "{}{}  {} / {}",
                    term::mv(x + 2, y + 2 + row),
                    fg,
                    su,
                    st
                ));
            }
        }
    } // show_swap

    out.push_str("\x1b[0m");
    out
}

fn gradient_color(gradient: &[String], pct: i64) -> &str {
    if gradient.is_empty() {
        return "";
    }
    &gradient[pct.clamp(0, 100) as usize]
}
