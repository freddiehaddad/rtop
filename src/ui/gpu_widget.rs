use crate::collect::CollectStatus;
use crate::domain::config_enums::TempScale;
use crate::domain::gpu::GpuInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::meter::Meter;
use crate::theme::{Theme, gradient_color};
use crate::theme_keys as tc;
use crate::tools;

use super::WidgetArea;

/// Per-frame view passed to [`draw`] for one GPU widget instance.
///
/// `index` and `custom_name` are per-instance (resolved at the
/// call site by indexing `config.gpu.custom_gpu_names[index]`).
/// `temp_scale` and `base_10` are shared cross-widget settings
/// lifted into this struct so the renderer doesn't need separate
/// `&CpuConfig` / `&UiConfig` borrows for one field each.
pub struct GpuFrame<'a> {
    pub index: usize,
    pub temp_scale: TempScale,
    pub custom_name: &'a str,
    pub base_10: bool,
}

/// Preferred intrinsic height for a GPU widget instance, in rows
/// (including borders). Fixed regardless of the snapshot — every
/// GPU widget is exactly [`crate::draw::layout::MIN_GPU_HEIGHT`]
/// tall.
pub fn preferred_height() -> usize {
    crate::draw::layout::MIN_GPU_HEIGHT
}

/// Format bytes into a short human-readable string (e.g., "10.8G").
fn fmt_bytes(bytes: u64, base10: bool) -> String {
    tools::floating_humanizer(bytes, true, 0, false, false, base10)
}

/// Format a clock speed for display (e.g., 2520 → "2.5GHz", 800 → "800MHz").
fn fmt_clock(mhz: u32) -> String {
    if mhz >= 1000 {
        format!("{:.1}GHz", mhz as f64 / 1000.0)
    } else {
        format!("{}MHz", mhz)
    }
}

/// Draw the GPU widget into an ANSI string.
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
    area: &WidgetArea,
    theme: &Theme,
    frame: &GpuFrame<'_>,
    status: &CollectStatus,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let border_color = theme.color(tc::GPU_WIDGET);
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

    let title = format!("gpu{}", frame.index);
    let num = super::GPU_KEY_BASE + frame.index as u8;
    let mut buf = AnsiBuffer::new();
    buf.text(&box_drawing::create_box(&box_drawing::BoxConfig {
        x,
        y,
        width,
        height,
        line_color: border_color,
        fill: true,
        title: &title,
        title2: "",
        num,
        rounded,
        hi_color: hi,
        title_color,
    }));

    super::draw_status_inset(&mut buf, status, &title, x, y, border_color, title_color);

    let inner_w = width.saturating_sub(4);
    let inner_h = height.saturating_sub(2);
    let content_x = x + 3;
    if inner_w < 10 || inner_h < 1 {
        return buf.finish();
    }

    // GPU name on the top border (right-aligned inset)
    let name_display = if frame.custom_name.is_empty() {
        &gpu.name
    } else {
        frame.custom_name
    };
    let max_name_w = inner_w.saturating_sub(title.len() + 6);
    let name_trunc: String = name_display.chars().take(max_name_w).collect();
    if !name_trunc.is_empty() {
        let inset = box_drawing::title_inset(&name_trunc, border_color, title_color, false);
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
            .color(fg)
            .text("GPU   ")
            .text(gpu_meter.render(gpu_pct))
            .color(gradient_color(grad_gpu, gpu_pct))
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
            .color(fg)
            .text("Clock ")
            .text(clock_meter.render(clock_pct))
            .color(gradient_color(grad_clock, clock_pct))
            .text(&tools::rjust(&fmt_clock(clock), val_w, true));
        row += 1;
    }

    // Row 3: Temperature
    let temp = gpu.temp.back().copied().unwrap_or(0);
    let (conv_temp, temp_unit) = crate::tools::celsius_to(temp, frame.temp_scale);
    let temp_pct = temp.clamp(0, 100) as i32;
    if row < inner_h {
        buf.mv(content_x, y + 2 + row)
            .color(fg)
            .text("Temp  ")
            .text(temp_meter.render(temp_pct))
            .color(gradient_color(grad_temp, temp_pct))
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
            .color(fg)
            .text("Watts ")
            .text(power_meter.render(pwr_pct))
            .color(gradient_color(grad_power, pwr_pct))
            .text(&tools::rjust(
                &format!("{:.0}W/{:.0}W", pwr_w, pwr_max_w),
                val_w,
                true,
            ));
        row += 1;
    }

    // Row 5: VRAM
    let vram_pct = gpu.mem_utilization_percent.back().copied().unwrap_or(0) as i32;
    let vram_used = fmt_bytes(gpu.mem_used, frame.base_10);
    let vram_total = fmt_bytes(gpu.mem_total, frame.base_10);
    if row < inner_h {
        buf.mv(content_x, y + 2 + row)
            .color(fg)
            .text("VRAM  ")
            .text(vram_meter.render(vram_pct))
            .color(gradient_color(grad_vram, vram_pct))
            .text(&tools::rjust(
                &format!("{}/{}", vram_used, vram_total),
                val_w,
                true,
            ));
    }

    buf.finish()
}

// ---------------------------------------------------------------------------
// Widget impl
// ---------------------------------------------------------------------------

/// GPU widget renderer. Single registry entry that handles every
/// per-instance `WidgetKind::Gpu(N)` — `kinds()` enumerates each
/// supported index and `render()` iterates them, drawing only the
/// instances present in the active layout AND backed by a real
/// device.
pub struct GpuWidget;

impl super::Widget for GpuWidget {
    fn kinds(&self) -> &'static [crate::domain::widget_kind::WidgetKind] {
        // Every supported GPU index is one of this widget's kinds.
        // The layout engine asks "which widget handles `Gpu(N)`?";
        // this slice answers all eight at once.
        const KINDS: &[crate::domain::widget_kind::WidgetKind] = &[
            crate::domain::widget_kind::WidgetKind::Gpu(0),
            crate::domain::widget_kind::WidgetKind::Gpu(1),
            crate::domain::widget_kind::WidgetKind::Gpu(2),
            crate::domain::widget_kind::WidgetKind::Gpu(3),
            crate::domain::widget_kind::WidgetKind::Gpu(4),
            crate::domain::widget_kind::WidgetKind::Gpu(5),
            crate::domain::widget_kind::WidgetKind::Gpu(6),
            crate::domain::widget_kind::WidgetKind::Gpu(7),
        ];
        KINDS
    }

    fn preferred_height(&self, _hints: &crate::draw::layout::LayoutHints) -> usize {
        preferred_height()
    }

    fn min_width(&self, _hints: &crate::draw::layout::LayoutHints) -> usize {
        crate::draw::layout::MIN_MEM_WIDTH
    }

    fn min_height(&self, _hints: &crate::draw::layout::LayoutHints) -> usize {
        preferred_height()
    }

    fn render(&self, params: &crate::app::RenderParams<'_>, output: &mut String) {
        // Iterate by actual GPU index n. Layout slots are keyed by
        // WidgetKind::Gpu(n), so a sparse selection (e.g. only
        // gpu1) renders the right device's snapshot with the
        // correct title and toggle key. Each GPU's snapshot is
        // independent — a missing slot simply means the per-device
        // collector hasn't published yet (or the device isn't
        // present at all, in which case `compose_hidden` will
        // already have hidden the widget).
        for n in 0..crate::config::MAX_GPUS {
            let kind = crate::domain::widget_kind::WidgetKind::Gpu(n as u8);
            // Per-instance dirty filter: only redraw the GPU(s)
            // whose snapshot actually changed. Pull-side
            // (`app::pull`) computes the change mask via
            // `GpuInfo::render_fingerprint` and marks each
            // changed kind dirty individually.
            if !params.dirty.is_widget_dirty(kind) {
                continue;
            }
            let Some(gpu_dim) = params.layout.dims_for(kind) else {
                continue;
            };
            let Some(snap) = params.gpu[n].as_deref() else {
                continue;
            };
            let area = super::WidgetArea::from_dim(gpu_dim, params.rounded);
            let custom_name = params
                .config
                .gpu
                .custom_gpu_names
                .get(n)
                .map(String::as_str)
                .unwrap_or("");
            let frame = GpuFrame {
                index: n,
                temp_scale: params.config.cpu.temp_scale,
                custom_name,
                base_10: params.config.ui.base_10_sizes,
            };
            output.push_str(&draw(&snap.info, &area, params.theme, &frame, &snap.status));
        }
    }
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

    fn make_area() -> WidgetArea {
        WidgetArea {
            x: 1,
            y: 1,
            width: 60,
            height: 7,
            rounded: true,
        }
    }

    fn make_frame() -> GpuFrame<'static> {
        GpuFrame {
            index: 0,
            temp_scale: TempScale::Celsius,
            custom_name: "",
            base_10: false,
        }
    }

    #[test]
    fn draw_contains_gpu_title() {
        let output = draw(
            &make_gpu_info(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
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
            &make_frame(),
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
            &make_frame(),
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
            &make_frame(),
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
            &make_frame(),
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
            &make_frame(),
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

    #[test]
    fn each_value_cell_uses_its_meter_gradient() {
        // Defends Option A: every value with a meter takes that meter's
        // gradient at the value's pct. Pre-fix every GPU value rendered in
        // MAIN_FG even though five distinct gradients existed for the meters.
        let theme = Theme::default();
        let info = make_gpu_info();
        let output = draw(
            &info,
            &make_area(),
            &theme,
            &make_frame(),
            &CollectStatus::Ok,
        );

        // GPU row: util = 78 % → GRAD_GPU[78] then "   78%".
        let grad_gpu = theme.gradient(tc::GRAD_GPU);
        assert!(
            output.contains(&format!("{}{:>10}", grad_gpu[78], "78%")),
            "GPU value should be GRAD_GPU[78] adjacent to right-justified value"
        );

        // Clock row: 2520 / 3000 = 84 %.
        let grad_clock = theme.gradient(tc::GRAD_GPU_CLOCK);
        assert!(
            output.contains(&format!("{}{:>10}", grad_clock[84], "2.5GHz")),
            "Clock value should be GRAD_GPU_CLOCK[84] adjacent to '2.5GHz'"
        );

        // Temp row: 65 → GRAD_TEMP[65].
        let grad_temp = theme.gradient(tc::GRAD_TEMP);
        assert!(
            output.contains(&format!("{}{:>10}", grad_temp[65], "65°C")),
            "Temp value should be GRAD_TEMP[65] adjacent to '65°C'"
        );

        // Watts row: 320000 / 450000 = 71 %, value '320W/450W'.
        let grad_power = theme.gradient(tc::GRAD_GPU_POWER);
        assert!(
            output.contains(&format!("{}{:>10}", grad_power[71], "320W/450W")),
            "Watts value should be GRAD_GPU_POWER[71] adjacent to '320W/450W'"
        );

        // VRAM row: 42 % from utilization slot.
        let grad_vram = theme.gradient(tc::GRAD_GPU_VRAM);
        assert!(
            output.contains(&grad_vram[42]),
            "VRAM value should be colored by GRAD_GPU_VRAM[42]"
        );
    }

    #[test]
    fn meter_row_labels_use_main_fg() {
        // Body label rule: GPU/Clock/Temp/Watts/VRAM labels render in MAIN_FG.
        let theme = Theme::default();
        let output = draw(
            &make_gpu_info(),
            &make_area(),
            &theme,
            &make_frame(),
            &CollectStatus::Ok,
        );
        let fg = theme.color(tc::MAIN_FG);
        for label in &["GPU   ", "Clock ", "Temp  ", "Watts ", "VRAM  "] {
            assert!(
                output.contains(&format!("{fg}{label}")),
                "gpu label {label:?} should be preceded by MAIN_FG"
            );
        }
    }
}
