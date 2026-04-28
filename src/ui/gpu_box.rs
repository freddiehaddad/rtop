use crate::collect::CollectStatus;
use crate::domain::gpu::GpuInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::meter::Meter;
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

use super::BoxArea;

/// Extracted settings for the GPU box, decoupled from Config.
pub struct GpuBoxSettings<'a> {
    pub index: usize,
    pub temp_scale: &'a str,
}

/// Format bytes into a short human-readable string (e.g., "10.8G").
fn fmt_bytes(bytes: u64) -> String {
    tools::floating_humanizer(bytes, true, 0, false, false, false)
}

/// Format a clock speed for display (e.g., 2520 → "2.5GHz", 800 → "800MHz").
fn fmt_clock(mhz: u32) -> String {
    if mhz >= 1000 {
        format!("{:.1}GHz", mhz as f64 / 1000.0)
    } else {
        format!("{}MHz", mhz)
    }
}

/// Draw the GPU box into an ANSI string.
///
/// Layout (5 content rows):
/// ╭─┐⁵gpu0┌────────────── NVIDIA GeForce RTX 4080 SUPER ╮
/// │ GPU   ■■■■■■■■■■■■░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  48% │
/// │ Clock ■■■■■░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  2.1GHz  │
/// │ Temp  ■■■■■■■■░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  42°C  │
/// │ Watts ■■■■■■■■■■■■■■■■░░░░░░░░░░░░░░░░░░  50W/352W   │
/// │ VRAM  ■■■■■■■■■■■░░░░░░░░░░░░░░░░░░░░░░░  4.5G/16G   │
/// ╰──────────────────────────────────────────────────────╯
pub fn draw(
    gpu: &GpuInfo,
    area: &BoxArea,
    theme: &Theme,
    settings: &GpuBoxSettings,
    status: &CollectStatus,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.color(tc::GPU_BOX);
    let fg = theme.color(tc::MAIN_FG);
    let hi = theme.color(tc::HI_FG);
    let title_color = theme.color(tc::TITLE);
    let meter_bg = theme.color(tc::METER_BG);
    // Per-row gradients — each GPU metric has its own semantic color
    let grad_gpu = theme.gradient(tc::GRAD_GPU);
    let grad_clock = theme.gradient(tc::GRAD_GPU_CLOCK);
    let grad_temp = theme.gradient(tc::GRAD_TEMP);
    let grad_power = theme.gradient(tc::GRAD_GPU_POWER);
    let grad_vram = theme.gradient(tc::GRAD_GPU_VRAM);

    let title = format!("gpu{}", settings.index);
    let num = 5u8;
    let mut buf = AnsiBuffer::new();
    buf.text(&box_drawing::create_box(&box_drawing::BoxConfig {
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

    super::draw_status_inset(&mut buf, status, &title, x, y, box_color, title_color);

    let inner_w = width.saturating_sub(4);
    let inner_h = height.saturating_sub(2);
    let content_x = x + 3;
    if inner_w < 10 || inner_h < 1 {
        return buf.finish();
    }

    // GPU name on the top border (right-aligned inset)
    let name_display = &gpu.name;
    let max_name_w = inner_w.saturating_sub(title.len() + 6);
    let name_trunc: String = name_display.chars().take(max_name_w).collect();
    if !name_trunc.is_empty() {
        let inset = box_drawing::title_inset(&name_trunc, box_color, title_color, false);
        let inset_x = box_drawing::right_inset_x(x, width, box_drawing::inset_width(&name_trunc));
        buf.mv(inset_x, y + 1).text(&inset);
    }

    // Consistent layout: label(6) + meter + value(val_w), like mem/disk
    let label_w = 6; // "GPU   ", "Clock ", etc.
    let val_w = 10; // right-aligned value column (fits "352W/352W" + 1 space)

    let meter_w = inner_w.saturating_sub(label_w + val_w).max(5);
    let gpu_meter = Meter::new(meter_w, grad_gpu, meter_bg);
    let clock_meter = Meter::new(meter_w, grad_clock, meter_bg);
    let temp_meter = Meter::new(meter_w, grad_temp, meter_bg);
    let power_meter = Meter::new(meter_w, grad_power, meter_bg);
    let vram_meter = Meter::new(meter_w, grad_vram, meter_bg);
    let mut row = 0;

    // Row 1: GPU utilization
    let gpu_pct = gpu.gpu_percent.utilization.back().copied().unwrap_or(0) as i32;
    if row < inner_h {
        buf.mv(content_x, y + 2 + row)
            .color(title_color)
            .text("GPU   ")
            .text(gpu_meter.render(gpu_pct))
            .color(fg)
            .text(&tools::rjust(&format!("{}%", gpu_pct), val_w, true));
        row += 1;
    }

    // Row 2: Clock speed
    let clock = gpu.gpu_clock_speed;
    let max_clock = gpu.gpu_max_clock_speed;
    let clock_pct = if max_clock > 0 {
        (clock as i32 * 100 / max_clock as i32).clamp(0, 100)
    } else {
        0
    };
    if row < inner_h {
        buf.mv(content_x, y + 2 + row)
            .color(title_color)
            .text("Clock ")
            .text(clock_meter.render(clock_pct))
            .color(fg)
            .text(&tools::rjust(&fmt_clock(clock), val_w, true));
        row += 1;
    }

    // Row 3: Temperature
    let temp = gpu.temp.back().copied().unwrap_or(0);
    let (conv_temp, temp_unit) = crate::tools::celsius_to(temp, settings.temp_scale);
    let temp_pct = temp.clamp(0, 100) as i32;
    if row < inner_h {
        buf.mv(content_x, y + 2 + row)
            .color(title_color)
            .text("Temp  ")
            .text(temp_meter.render(temp_pct))
            .color(fg)
            .text(&tools::rjust(
                &format!("{}{}", conv_temp, temp_unit),
                val_w,
                true,
            ));
        row += 1;
    }

    // Row 4: Power
    let pwr_w = gpu.pwr_usage as f64 / 1000.0;
    let pwr_max_w = gpu.pwr_max_usage as f64 / 1000.0;
    let pwr_pct = if gpu.pwr_max_usage > 0 {
        (gpu.pwr_usage * 100 / gpu.pwr_max_usage).clamp(0, 100) as i32
    } else {
        0
    };
    if row < inner_h {
        buf.mv(content_x, y + 2 + row)
            .color(title_color)
            .text("Watts ")
            .text(power_meter.render(pwr_pct))
            .color(fg)
            .text(&tools::rjust(
                &format!("{:.0}W/{:.0}W", pwr_w, pwr_max_w),
                val_w,
                true,
            ));
        row += 1;
    }

    // Row 5: VRAM
    let vram_pct = gpu.mem_utilization_percent.back().copied().unwrap_or(0) as i32;
    let vram_used = fmt_bytes(gpu.mem_used);
    let vram_total = fmt_bytes(gpu.mem_total);
    if row < inner_h {
        buf.mv(content_x, y + 2 + row)
            .color(title_color)
            .text("VRAM  ")
            .text(vram_meter.render(vram_pct))
            .color(fg)
            .text(&tools::rjust(
                &format!("{}/{}", vram_used, vram_total),
                val_w,
                true,
            ));
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
            gpu_max_clock_speed: 3000,
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
            height: 7,
            rounded: true,
        }
    }

    fn make_settings() -> GpuBoxSettings<'static> {
        GpuBoxSettings {
            index: 0,
            temp_scale: "celsius",
        }
    }

    #[test]
    fn draw_contains_gpu_title() {
        let output = draw(
            &make_gpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("gpu0"), "output should contain 'gpu0' title");
    }

    #[test]
    fn draw_contains_gpu_name() {
        let output = draw(
            &make_gpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("Test GPU RTX 5090"),
            "output should contain GPU name 'Test GPU RTX 5090'"
        );
    }

    #[test]
    fn draw_contains_all_five_rows() {
        let output = draw(
            &make_gpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("GPU"), "should contain GPU row");
        assert!(plain.contains("Clock"), "should contain Clock row");
        assert!(plain.contains("Temp"), "should contain Temp row");
        assert!(plain.contains("Watts"), "should contain Watts row");
        assert!(plain.contains("VRAM"), "should contain VRAM row");
    }

    #[test]
    fn draw_contains_clock_speed() {
        let output = draw(
            &make_gpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("2.5GHz"),
            "should show current clock speed: got {plain}"
        );
    }

    #[test]
    fn draw_contains_power_ratio() {
        let output = draw(
            &make_gpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("320W/450W"),
            "should show power ratio: got {plain}"
        );
    }

    #[test]
    fn draw_contains_vram_ratio() {
        let output = draw(
            &make_gpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        // floating_humanizer output
        assert!(
            plain.contains("/24"),
            "should show vram used/total: got {plain}"
        );
    }

    #[test]
    fn fmt_clock_formats_correctly() {
        assert_eq!(fmt_clock(2100), "2.1GHz");
        assert_eq!(fmt_clock(2520), "2.5GHz");
        assert_eq!(fmt_clock(3000), "3.0GHz");
        assert_eq!(fmt_clock(800), "800MHz");
        assert_eq!(fmt_clock(0), "0MHz");
    }
}
