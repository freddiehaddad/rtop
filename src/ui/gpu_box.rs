use crate::domain::gpu::GpuInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::meter::Meter;
use crate::theme::Theme;

/// Format bytes into a short human-readable string (e.g., "10.8G").
fn fmt_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.1}G", b / GIB)
    } else {
        format!("{:.0}M", b / MIB)
    }
}

use super::BoxArea;

/// Extracted settings for the GPU box, decoupled from Config.
pub struct GpuBoxSettings<'a> {
    pub temp_scale: &'a str,
}

/// Draw the GPU box into an ANSI string.
///
/// Layout (4 rows):
/// ╭─┐⁵gpu0┌─ NVIDIA RTX 4090 ───────────────────────────╮
/// │ GPU 78% ■■■■■■■■■■■■■■░░░░  🌡 65°C  ⚡ 320W / 450W  │
/// │ VRAM 45% ■■■■■■■■░░░░░░░░░  10.8G / 24.0G  2520 MHz  │
/// ╰──────────────────────────────────────────────────────╯
pub fn draw(
    gpu: &GpuInfo,
    index: usize,
    area: &BoxArea,
    theme: &Theme,
    settings: &GpuBoxSettings,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.c("gpu_box");
    let fg = theme.c("main_fg");
    let hi = theme.c("hi_fg");
    let title_color = theme.c("title");
    let meter_bg = theme.c("meter_bg");
    let cpu_gradient = theme.g("cpu");

    let title = format!("gpu{index}");
    let num = 5u8;
    let mut buf = AnsiBuffer::new();
    buf.raw(&box_drawing::create_box(&box_drawing::BoxConfig {
        x,
        y,
        width,
        height,
        line_color: box_color,
        fill: true,
        title: &title,
        title2: "",
        num,
        rounded,
        hi_color: hi,
        title_color,
    }));

    let inner_w = width.saturating_sub(2);
    if inner_w < 10 || height < 3 {
        return buf.finish();
    }

    // GPU name on the top border after the title
    let name_display = &gpu.name;
    let name_max = inner_w.saturating_sub(title.len() + 6);
    let name_trunc: String = name_display.chars().take(name_max).collect();
    if !name_trunc.is_empty() {
        let name_x = x + title.len() + 6;
        buf.mv(name_x, y + 1).raw(&box_drawing::title_inset(
            &name_trunc,
            box_color,
            title_color,
            false,
        ));
    }

    // Row 1: GPU utilization meter + temperature + power
    let gpu_pct = gpu.gpu_percent.utilization.back().copied().unwrap_or(0);
    let temp = gpu.temp.back().copied().unwrap_or(0);
    let (conv_temp, temp_unit) = crate::tools::celsius_to(temp, settings.temp_scale);
    let pwr_w = gpu.pwr_usage as f64 / 1000.0;
    let pwr_max_w = gpu.pwr_max_usage as f64 / 1000.0;

    let label = format!(" GPU {gpu_pct:>3}% ");
    let suffix = format!("  {conv_temp}{temp_unit}  {pwr_w:.0}W/{pwr_max_w:.0}W ");
    let meter_w = inner_w.saturating_sub(label.len() + suffix.len());

    if height >= 4 {
        // We have 2 inner rows
        let meter = Meter::new(meter_w.max(1), cpu_gradient, meter_bg);
        buf.mv(x + 2, y + 2)
            .color(fg)
            .text(&label)
            .raw(meter.render(gpu_pct as i32))
            .color(fg)
            .text(&suffix);

        // Row 2: VRAM usage meter + VRAM total + clock speed
        let vram_pct = gpu.mem_utilization_percent.back().copied().unwrap_or(0);
        let vram_used = fmt_bytes(gpu.mem_used);
        let vram_total = fmt_bytes(gpu.mem_total);
        let clock = gpu.gpu_clock_speed;

        let vlabel = format!(" VRAM {vram_pct:>3}% ");
        let vsuffix = format!("  {vram_used}/{vram_total}  {clock} MHz ");
        let vmeter_w = inner_w.saturating_sub(vlabel.len() + vsuffix.len());
        let vmeter = Meter::new(vmeter_w.max(1), cpu_gradient, meter_bg);
        buf.mv(x + 2, y + 3)
            .color(fg)
            .text(&vlabel)
            .raw(vmeter.render(vram_pct as i32))
            .color(fg)
            .text(&vsuffix);
    } else {
        // Only 1 inner row — compact view
        let meter = Meter::new(meter_w.max(1), cpu_gradient, meter_bg);
        buf.mv(x + 2, y + 2)
            .color(fg)
            .text(&label)
            .raw(meter.render(gpu_pct as i32))
            .color(fg)
            .text(&suffix);
    }

    buf.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gpu::GpuPercent;
    use std::collections::VecDeque;

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

    fn make_gpu_info() -> GpuInfo {
        GpuInfo {
            name: "Test GPU RTX 5090".into(),
            gpu_percent: GpuPercent {
                utilization: VecDeque::from([78]),
                vram: VecDeque::from([45]),
                power: VecDeque::from([70]),
            },
            gpu_clock_speed: 2520,
            pwr_usage: 320_000,
            pwr_max_usage: 450_000,
            temp: VecDeque::from([65]),
            mem_total: 24 * 1024 * 1024 * 1024,
            mem_used: 10 * 1024 * 1024 * 1024,
            mem_utilization_percent: VecDeque::from([42]),
        }
    }

    fn make_area() -> BoxArea {
        BoxArea {
            x: 1,
            y: 1,
            width: 60,
            height: 4,
            rounded: true,
        }
    }

    fn make_settings() -> GpuBoxSettings<'static> {
        GpuBoxSettings {
            temp_scale: "celsius",
        }
    }

    #[test]
    fn draw_contains_gpu_title() {
        let output = draw(
            &make_gpu_info(),
            0,
            &make_area(),
            &Theme::default(),
            &make_settings(),
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("gpu0"), "output should contain 'gpu0' title");
    }

    #[test]
    fn draw_contains_gpu_name() {
        let output = draw(
            &make_gpu_info(),
            0,
            &make_area(),
            &Theme::default(),
            &make_settings(),
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("Test GPU RTX 5090"),
            "output should contain GPU name 'Test GPU RTX 5090'"
        );
    }
}
