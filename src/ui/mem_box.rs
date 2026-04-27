use crate::domain::memory::MemInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::meter::Meter;
use crate::theme::Theme;
use crate::theme_keys as tc;
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
    let box_color = theme.c(tc::MEM_BOX);
    let fg = theme.c(tc::MAIN_FG);
    let title_color = theme.c(tc::TITLE);
    let hi = theme.c(tc::HI_FG);
    let used_grad = theme.g(tc::GRAD_USED);
    let free_grad = theme.g(tc::GRAD_FREE);
    let cached_grad = theme.g(tc::GRAD_CACHED);
    let avail_grad = theme.g(tc::GRAD_AVAILABLE);

    let inner_h = height.saturating_sub(2);

    let mut buf = AnsiBuffer::new();
    buf.text(&box_drawing::create_box(&box_drawing::BoxConfig {
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
    }));

    let total_bytes = mem.stats.used + mem.stats.available;
    let meter_area = width.saturating_sub(4);
    let label_w = 6;
    let meter_w = meter_area.saturating_sub(label_w + 6).max(5);
    let meter_bg = theme.c(tc::METER_BG);
    let used_meter = Meter::new(meter_w, used_grad, meter_bg);
    let avail_meter = Meter::new(meter_w, avail_grad, meter_bg);
    let cached_meter = Meter::new(meter_w, cached_grad, meter_bg);
    let free_meter = Meter::new(meter_w, free_grad, meter_bg);
    let mut row = 0;

    // Used
    let used = mem.stats.used;
    let used_pct = (used * 100).checked_div(total_bytes).unwrap_or(0) as i32;
    let used_color = gradient_color(used_grad, used_pct as i64);
    let used_str = tools::floating_humanizer(used, true, 0, false, false, false);
    if row < inner_h {
        buf.mv(x + 2, y + 2 + row)
            .color(title_color)
            .text("Used  ")
            .text(used_meter.render(used_pct))
            .text("  ")
            .color(used_color)
            .text(&used_str);
        row += 1;
    }

    // Available
    let avail = mem.stats.available;
    let avail_pct = (avail * 100).checked_div(total_bytes).unwrap_or(0) as i32;
    let avail_color = gradient_color(avail_grad, avail_pct as i64);
    let avail_str = tools::floating_humanizer(avail, true, 0, false, false, false);
    if row < inner_h {
        buf.mv(x + 2, y + 2 + row)
            .color(title_color)
            .text("Avail ")
            .text(avail_meter.render(avail_pct))
            .text("  ")
            .color(avail_color)
            .text(&avail_str);
        row += 1;
    }

    // Cached
    let cached = mem.stats.cached;
    if cached > 0 && row < inner_h {
        let cached_pct = (cached * 100).checked_div(total_bytes).unwrap_or(0) as i32;
        let cache_color = gradient_color(cached_grad, cached_pct as i64);
        let cached_str = tools::floating_humanizer(cached, true, 0, false, false, false);
        buf.mv(x + 2, y + 2 + row)
            .color(title_color)
            .text("Cache ")
            .text(cached_meter.render(cached_pct))
            .text("  ")
            .color(cache_color)
            .text(&cached_str);
        row += 1;
    }

    // Free
    let free = mem.stats.free;
    let free_pct = (free * 100).checked_div(total_bytes).unwrap_or(0) as i32;
    let free_color = gradient_color(free_grad, free_pct as i64);
    let free_str = tools::floating_humanizer(free, true, 0, false, false, false);
    if row < inner_h {
        buf.mv(x + 2, y + 2 + row)
            .color(title_color)
            .text("Free  ")
            .text(free_meter.render(free_pct))
            .text("  ")
            .color(free_color)
            .text(&free_str);
        row += 1;
    }

    // Blank line before swap
    if show_swap {
        if row < inner_h {
            row += 1;
        }

        // Swap
        let swap_used = mem.stats.swap_used;
        let swap_total = mem.stats.swap_total;
        if swap_total > 0 && row < inner_h {
            let swap_pct = (swap_used * 100 / swap_total.max(1)) as i32;
            let swap_str = tools::floating_humanizer(swap_used, true, 0, false, false, false);
            buf.mv(x + 2, y + 2 + row)
                .color(title_color)
                .text("Swap  ")
                .text(used_meter.render(swap_pct))
                .text("  ")
                .color(fg)
                .text(&swap_str);
            row += 1;

            // Swap total line
            if row < inner_h {
                let su = tools::floating_humanizer(swap_used, true, 0, false, false, false);
                let st = tools::floating_humanizer(swap_total, true, 0, false, false, false);
                let swap_line = format!("  {} / {}", su, st);
                buf.mv(x + 2, y + 2 + row).color(fg).text(&swap_line);
            }
        }
    } // show_swap

    buf.finish()
}

fn gradient_color(gradient: &[String], pct: i64) -> &str {
    if gradient.is_empty() {
        return "";
    }
    &gradient[pct.clamp(0, 100) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::{MemPercent, MemStats};

    fn strip_ansi(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut in_escape = false;
        for ch in s.chars() {
            if in_escape {
                if ch.is_ascii_alphabetic() || ch == 'm' {
                    in_escape = false;
                }
                continue;
            }
            if ch == '\x1b' {
                in_escape = true;
                continue;
            }
            result.push(ch);
        }
        result
    }

    const GIB: u64 = 1024 * 1024 * 1024;

    fn make_mem_info() -> MemInfo {
        MemInfo {
            stats: MemStats {
                used: 8 * GIB,
                available: 8 * GIB,
                cached: 2 * GIB,
                free: 6 * GIB,
                swap_total: 4 * GIB,
                swap_used: 1 * GIB,
                swap_free: 3 * GIB,
            },
            percent: MemPercent::default(),
        }
    }

    fn make_area() -> BoxArea {
        BoxArea {
            x: 1,
            y: 1,
            width: 40,
            height: 12,
            rounded: true,
        }
    }

    #[test]
    fn draw_contains_mem_title() {
        let output = draw(&make_mem_info(), &make_area(), &Theme::default(), true);
        let plain = strip_ansi(&output);
        assert!(plain.contains("mem"), "output should contain 'mem' title");
    }

    #[test]
    fn draw_contains_used_label() {
        let output = draw(&make_mem_info(), &make_area(), &Theme::default(), true);
        let plain = strip_ansi(&output);
        assert!(plain.contains("Used"), "output should contain 'Used' label");
    }

    #[test]
    fn draw_shows_swap_when_enabled() {
        let output = draw(&make_mem_info(), &make_area(), &Theme::default(), true);
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("Swap"),
            "output should contain 'Swap' when show_swap=true"
        );
    }

    #[test]
    fn draw_hides_swap_when_disabled() {
        let output = draw(&make_mem_info(), &make_area(), &Theme::default(), false);
        let plain = strip_ansi(&output);
        assert!(
            !plain.contains("Swap"),
            "output should not contain 'Swap' when show_swap=false"
        );
    }
}
