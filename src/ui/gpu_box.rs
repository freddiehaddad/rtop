#[cfg(feature = "gpu")]
use crate::domain::gpu::GpuInfo;
#[cfg(feature = "gpu")]
use crate::draw::box_drawing;

/// Draw the GPU box into an ANSI string (only available with "gpu" feature).
#[cfg(feature = "gpu")]
pub fn draw(
    gpu: &GpuInfo,
    index: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    rounded: bool,
) -> String {
    let title = format!("gpu{}", index);
    let mut out = box_drawing::create_box(x, y, width, height, "", false, &title, "", 0, rounded);

    // GPU name
    let name = crate::tools::uresize(&gpu.name, width.saturating_sub(4), false);
    out.push_str(&format!("\x1b[{};{}H{}", y + 2, x + 2, name));

    // GPU utilization
    if let Some(totals) = gpu.gpu_percent.get("gpu-totals") {
        if let Some(&pct) = totals.back() {
            out.push_str(&format!("\x1b[{};{}HGPU: {}%", y + 3, x + 2, pct));
        }
    }

    out
}
