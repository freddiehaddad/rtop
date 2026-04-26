use crate::domain::gpu::GpuInfo;
use crate::draw::box_drawing;
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

/// Draw the GPU box into an ANSI string.
///
/// Layout (4 rows):
/// ╭─┐⁵gpu0┌─ NVIDIA RTX 4090 ───────────────────────────╮
/// │ GPU 78% ■■■■■■■■■■■■■■░░░░  🌡 65°C  ⚡ 320W / 450W  │
/// │ VRAM 45% ■■■■■■■■░░░░░░░░░  10.8G / 24.0G  2520 MHz  │
/// ╰──────────────────────────────────────────────────────╯
#[allow(clippy::too_many_arguments)]
pub fn draw(
    gpu: &GpuInfo,
    index: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    rounded: bool,
    theme: &Theme,
) -> String {
    let box_color = theme.c("cpu_box");
    let fg = theme.c("main_fg");
    let meter_bg = theme.c("meter_bg");
    let cpu_gradient = theme.g("cpu");

    let title = format!("gpu{index}");
    let num = 5u8;
    let mut out =
        box_drawing::create_box(x, y, width, height, box_color, true, &title, "", num, rounded);

    let inner_w = width.saturating_sub(2);
    if inner_w < 10 || height < 3 {
        out.push_str("\x1b[0m");
        return out;
    }

    // GPU name on the top border after the title
    let name_display = &gpu.name;
    let name_max = inner_w.saturating_sub(title.len() + 6);
    let name_trunc: String = name_display.chars().take(name_max).collect();
    if !name_trunc.is_empty() {
        let name_x = x + title.len() + 6;
        out.push_str(&format!(
            "\x1b[{};{}H{}{} {} {}{}",
            y + 1,
            name_x,
            box_color,
            box_drawing::title_syms::TITLE_LEFT,
            name_trunc,
            box_drawing::title_syms::TITLE_RIGHT,
            "\x1b[0m",
        ));
    }

    // Row 1: GPU utilization meter + temperature + power
    let gpu_pct = gpu
        .gpu_percent
        .get("gpu-totals")
        .and_then(|v| v.back())
        .copied()
        .unwrap_or(0);
    let temp = gpu.temp.back().copied().unwrap_or(0);
    let pwr_w = gpu.pwr_usage as f64 / 1000.0;
    let pwr_max_w = gpu.pwr_max_usage as f64 / 1000.0;

    let label = format!(" GPU {gpu_pct:>3}% ");
    let suffix = format!("  {temp}°C  {pwr_w:.0}W/{pwr_max_w:.0}W ");
    let meter_w = inner_w.saturating_sub(label.len() + suffix.len());

    if height >= 4 {
        // We have 2 inner rows
        let meter = Meter::new(meter_w.max(1), cpu_gradient, meter_bg);
        out.push_str(&format!(
            "\x1b[{};{}H{}{}{}{}{}",
            y + 2,
            x + 2,
            fg,
            label,
            meter.render(gpu_pct as i32),
            fg,
            suffix,
        ));

        // Row 2: VRAM usage meter + VRAM total + clock speed
        let vram_pct = gpu
            .mem_utilization_percent
            .back()
            .copied()
            .unwrap_or(0);
        let vram_used = fmt_bytes(gpu.mem_used);
        let vram_total = fmt_bytes(gpu.mem_total);
        let clock = gpu.gpu_clock_speed;

        let vlabel = format!(" VRAM {vram_pct:>3}% ");
        let vsuffix = format!("  {vram_used}/{vram_total}  {clock} MHz ");
        let vmeter_w = inner_w.saturating_sub(vlabel.len() + vsuffix.len());
        let vmeter = Meter::new(vmeter_w.max(1), cpu_gradient, meter_bg);
        out.push_str(&format!(
            "\x1b[{};{}H{}{}{}{}{}",
            y + 3,
            x + 2,
            fg,
            vlabel,
            vmeter.render(vram_pct as i32),
            fg,
            vsuffix,
        ));
    } else {
        // Only 1 inner row — compact view
        let meter = Meter::new(meter_w.max(1), cpu_gradient, meter_bg);
        out.push_str(&format!(
            "\x1b[{};{}H{}{}{}{}{}",
            y + 2,
            x + 2,
            fg,
            label,
            meter.render(gpu_pct as i32),
            fg,
            suffix,
        ));
    }

    out.push_str("\x1b[0m");
    out
}
