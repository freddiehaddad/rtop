use crate::collect::CollectStatus;
use crate::domain::memory::MemInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::meter::Meter;
use crate::theme::{Theme, gradient_color};
use crate::theme_keys as tc;
use crate::tools;

use super::BoxArea;

/// Extracted settings for the memory box, decoupled from Config.
pub struct MemBoxSettings {
    pub show_swap: bool,
    pub base_10: bool,
}

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
pub fn draw(
    mem: &MemInfo,
    area: &BoxArea,
    theme: &Theme,
    settings: &MemBoxSettings,
    status: &CollectStatus,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.color(tc::MEM_BOX);
    let title_color = theme.color(tc::TITLE);
    let hi = theme.color(tc::HI_FG);
    let used_grad = theme.gradient(tc::GRAD_USED);
    let free_grad = theme.gradient(tc::GRAD_FREE);
    let cached_grad = theme.gradient(tc::GRAD_CACHED);
    let avail_grad = theme.gradient(tc::GRAD_AVAILABLE);

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
        num: super::MEM_KEY,
        rounded,
        hi_color: hi,
        title_color,
    }));

    super::draw_status_inset(&mut buf, status, "mem", x, y, box_color, title_color);

    let total_bytes = mem.stats.used + mem.stats.available;

    // Total memory inset on top border (like CPU frequency)
    let total_str = tools::floating_humanizer(total_bytes, true, 0, false, false, settings.base_10);
    let inset = box_drawing::title_inset(&total_str, box_color, title_color, false);
    let inset_x = box_drawing::right_inset_x(x, width, box_drawing::inset_width(&total_str));
    buf.mv(inset_x, y + 1).text(&inset);

    // Content area: width - 4 (border + space on each side)
    // Content starts at x + 3 (border at x+1, space at x+2)
    let val_w = 5; // right-aligned value column
    let label_w = 6; // "Used  ", "Avail ", etc.
    let inner_w = width.saturating_sub(4);
    let meter_w = inner_w.saturating_sub(label_w + val_w).max(5);
    let content_x = x + 3;
    let meter_bg = theme.color(tc::METER_BG);
    let used_meter = Meter::new(meter_w, used_grad, meter_bg);
    let avail_meter = Meter::new(meter_w, avail_grad, meter_bg);
    let cached_meter = Meter::new(meter_w, cached_grad, meter_bg);
    let free_meter = Meter::new(meter_w, free_grad, meter_bg);
    let mut row = 0;

    // Helper: render " Label meter  Value " with right-aligned value
    let render_row = |buf: &mut AnsiBuffer,
                      label: &str,
                      meter_str: &str,
                      value: &str,
                      label_color: &str,
                      val_color: &str,
                      rx: usize,
                      ry: usize| {
        buf.mv(rx, ry)
            .color(label_color)
            .text(label)
            .text(meter_str)
            .color(val_color)
            .text(&tools::rjust(value, val_w, false));
    };

    // Used
    let used = mem.stats.used;
    let used_pct = (used * 100).checked_div(total_bytes).unwrap_or(0) as i32;
    let used_color = gradient_color(used_grad, used_pct);
    let used_str = tools::floating_humanizer(used, true, 0, false, false, settings.base_10);
    if row < inner_h {
        render_row(
            &mut buf,
            "Used  ",
            used_meter.render(used_pct),
            &used_str,
            title_color,
            used_color,
            content_x,
            y + 2 + row,
        );
        row += 1;
    }

    // Available
    let avail = mem.stats.available;
    let avail_pct = (avail * 100).checked_div(total_bytes).unwrap_or(0) as i32;
    let avail_color = gradient_color(avail_grad, avail_pct);
    let avail_str = tools::floating_humanizer(avail, true, 0, false, false, settings.base_10);
    if row < inner_h {
        render_row(
            &mut buf,
            "Avail ",
            avail_meter.render(avail_pct),
            &avail_str,
            title_color,
            avail_color,
            content_x,
            y + 2 + row,
        );
        row += 1;
    }

    // Cached
    let cached = mem.stats.cached;
    if cached > 0 && row < inner_h {
        let cached_pct = (cached * 100).checked_div(total_bytes).unwrap_or(0) as i32;
        let cache_color = gradient_color(cached_grad, cached_pct);
        let cached_str = tools::floating_humanizer(cached, true, 0, false, false, settings.base_10);
        render_row(
            &mut buf,
            "Cache ",
            cached_meter.render(cached_pct),
            &cached_str,
            title_color,
            cache_color,
            content_x,
            y + 2 + row,
        );
        row += 1;
    }

    // Free
    let free = mem.stats.free;
    let free_pct = (free * 100).checked_div(total_bytes).unwrap_or(0) as i32;
    let free_color = gradient_color(free_grad, free_pct);
    let free_str = tools::floating_humanizer(free, true, 0, false, false, settings.base_10);
    if row < inner_h {
        render_row(
            &mut buf,
            "Free  ",
            free_meter.render(free_pct),
            &free_str,
            title_color,
            free_color,
            content_x,
            y + 2 + row,
        );
        row += 1;
    }

    // Swap (no blank line, no separate total line — meter shows ratio)
    if settings.show_swap {
        let swap_used = mem.stats.swap_used;
        let swap_total = mem.stats.swap_total;
        let swap_pct = if swap_total > 0 {
            (swap_used * 100 / swap_total.max(1)) as i32
        } else {
            0
        };
        let swap_color = gradient_color(used_grad, swap_pct);
        let swap_str =
            tools::floating_humanizer(swap_used, true, 0, false, false, settings.base_10);
        if row < inner_h {
            render_row(
                &mut buf,
                "Swap  ",
                used_meter.render(swap_pct),
                &swap_str,
                title_color,
                swap_color,
                content_x,
                y + 2 + row,
            );
        }
    }

    buf.finish()
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
                swap_used: GIB,
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
        let output = draw(
            &make_mem_info(),
            &make_area(),
            &Theme::default(),
            &MemBoxSettings {
                show_swap: true,
                base_10: false,
            },
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("mem"), "output should contain 'mem' title");
    }

    #[test]
    fn draw_contains_used_label() {
        let output = draw(
            &make_mem_info(),
            &make_area(),
            &Theme::default(),
            &MemBoxSettings {
                show_swap: true,
                base_10: false,
            },
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("Used"), "output should contain 'Used' label");
    }

    #[test]
    fn draw_shows_swap_when_enabled() {
        let output = draw(
            &make_mem_info(),
            &make_area(),
            &Theme::default(),
            &MemBoxSettings {
                show_swap: true,
                base_10: false,
            },
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("Swap"),
            "output should contain 'Swap' when show_swap=true"
        );
    }

    #[test]
    fn draw_hides_swap_when_disabled() {
        let output = draw(
            &make_mem_info(),
            &make_area(),
            &Theme::default(),
            &MemBoxSettings {
                show_swap: false,
                base_10: false,
            },
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            !plain.contains("Swap"),
            "output should not contain 'Swap' when show_swap=false"
        );
    }

    #[test]
    fn swap_value_is_colored_by_used_gradient() {
        // Defends Option A: every value with a meter takes the meter's
        // gradient at the value's pct. Swap meter uses GRAD_USED, so swap
        // value must too. Pre-fix the swap value used MAIN_FG while the four
        // rows above used their gradients — the only inconsistency in mem.
        let theme = Theme::default();
        // 1 GiB used / 4 GiB total = 25 %.
        let info = make_mem_info();
        let output = draw(
            &info,
            &make_area(),
            &theme,
            &MemBoxSettings {
                show_swap: true,
                base_10: false,
            },
            &CollectStatus::Ok,
        );
        let used_grad = theme.gradient(tc::GRAD_USED);
        let expected = format!("{}{}", used_grad[25], " 1.0G");
        assert!(
            output.contains(&expected),
            "swap value cell should be GRAD_USED[25] immediately followed by ' 1.0G'; got:\n{output}"
        );
    }
}
