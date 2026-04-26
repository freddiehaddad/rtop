/// A 1-line horizontal progress bar using ■ characters.
#[derive(Debug, Clone)]
pub struct Meter {
    width: usize,
    cache: Vec<String>,
}

/// The character used for filled portions of the meter.
pub const METER_CHAR: char = '■';

impl Meter {
    /// Create a new meter with the given display width.
    /// Pre-computes 101 cached renderings (0-100%).
    pub fn new(width: usize) -> Self {
        let mut cache = Vec::with_capacity(101);
        for pct in 0..=100 {
            let filled = (width as u64 * pct as u64 / 100) as usize;
            let empty = width - filled;
            let mut s = String::with_capacity(width * 3);
            for _ in 0..filled {
                s.push(METER_CHAR);
            }
            for _ in 0..empty {
                s.push('░');
            }
            cache.push(s);
        }
        Self { width, cache }
    }

    /// Render the meter at the given percentage (0-100).
    pub fn render(&self, value: i32) -> &str {
        let clamped = value.clamp(0, 100) as usize;
        &self.cache[clamped]
    }

    pub fn width(&self) -> usize {
        self.width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_0_percent_empty() {
        let meter = Meter::new(10);
        let result = meter.render(0);
        assert!(!result.contains(METER_CHAR));
    }

    #[test]
    fn render_100_percent_full() {
        let meter = Meter::new(10);
        let result = meter.render(100);
        assert_eq!(result.matches(METER_CHAR).count(), 10);
    }

    #[test]
    fn render_50_percent_half() {
        let meter = Meter::new(10);
        let result = meter.render(50);
        assert_eq!(result.matches(METER_CHAR).count(), 5);
    }

    #[test]
    fn render_cache_hit() {
        let meter = Meter::new(20);
        let r1 = meter.render(75);
        let r2 = meter.render(75);
        assert_eq!(r1, r2);
        // Same pointer (cached)
        assert!(std::ptr::eq(r1, r2));
    }

    #[test]
    fn render_clamps_out_of_range() {
        let meter = Meter::new(10);
        let r_neg = meter.render(-5);
        let r_over = meter.render(150);
        assert_eq!(r_neg, meter.render(0));
        assert_eq!(r_over, meter.render(100));
    }
}
