/// A 1-line horizontal progress bar.
///
/// btop uses the same character (■) for both filled and empty portions.
/// Filled characters get per-position gradient colors.
/// Empty characters use the `meter_bg` color.
#[derive(Debug, Clone)]
pub struct Meter {
    cache: Vec<String>,
}

/// The character used for the meter bar (both filled and empty).
pub const METER_CHAR: char = '■';

impl Meter {
    /// Create a new meter with the given display width.
    /// Pre-computes 101 cached renderings (0-100%).
    /// `gradient` is the theme gradient (101 color escapes).
    /// `meter_bg` is the ANSI escape for the empty portion color.
    pub fn new(width: usize, gradient: &[String], meter_bg: &str) -> Self {
        let mut cache = Vec::with_capacity(101);
        for pct in 0..=100i32 {
            let filled = (width as u64 * pct as u64 / 100) as usize;
            let empty = width - filled;
            let mut s = String::with_capacity(width * 20);

            // Filled portion — each character gets its gradient color
            for i in 0..filled {
                let color_idx = (i as f64 * 100.0 / width as f64).round() as usize;
                if !gradient.is_empty() {
                    s.push_str(&gradient[color_idx.min(100)]);
                }
                s.push(METER_CHAR);
            }

            // Empty portion — all use meter_bg color
            if empty > 0 {
                s.push_str(meter_bg);
                for _ in 0..empty {
                    s.push(METER_CHAR);
                }
            }

            s.push_str(crate::term::RESET);
            cache.push(s);
        }
        Self { cache }
    }

    /// Render the meter at the given percentage (0-100).
    pub fn render(&self, value: i32) -> &str {
        let clamped = value.clamp(0, 100) as usize;
        &self.cache[clamped]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_gradient() -> Vec<String> {
        (0..=100).map(|_| String::new()).collect()
    }

    #[test]
    fn render_0_percent_all_meter_chars() {
        let meter = Meter::new(10, &test_gradient(), "");
        let result = meter.render(0);
        assert_eq!(result.matches(METER_CHAR).count(), 10);
    }

    #[test]
    fn render_100_percent_all_meter_chars() {
        let meter = Meter::new(10, &test_gradient(), "");
        let result = meter.render(100);
        assert_eq!(result.matches(METER_CHAR).count(), 10);
    }

    #[test]
    fn render_50_percent_all_meter_chars() {
        let meter = Meter::new(10, &test_gradient(), "");
        let result = meter.render(50);
        assert_eq!(result.matches(METER_CHAR).count(), 10);
    }

    #[test]
    fn render_cache_hit() {
        let meter = Meter::new(20, &test_gradient(), "");
        let r1 = meter.render(75);
        let r2 = meter.render(75);
        assert_eq!(r1, r2);
        assert!(std::ptr::eq(r1, r2));
    }

    #[test]
    fn render_clamps_out_of_range() {
        let meter = Meter::new(10, &test_gradient(), "");
        let r_neg = meter.render(-5);
        let r_over = meter.render(150);
        assert_eq!(r_neg, meter.render(0));
        assert_eq!(r_over, meter.render(100));
    }
}
