#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CounterDelta {
    Delta(u64),
    Reset,
}

pub(crate) fn counter_delta(current: u64, previous: u64) -> CounterDelta {
    if current >= previous {
        CounterDelta::Delta(current - previous)
    } else {
        CounterDelta::Reset
    }
}

pub(crate) fn bytes_per_sec(current: u64, previous: u64, elapsed_secs: f64) -> u64 {
    if previous == 0 || elapsed_secs <= 0.0 || !elapsed_secs.is_finite() {
        return 0;
    }

    let CounterDelta::Delta(delta) = counter_delta(current, previous) else {
        return 0;
    };

    (delta as f64 / elapsed_secs).clamp(0.0, u64::MAX as f64) as u64
}

pub(crate) fn percent_u64(part: u64, total: u64) -> i64 {
    if total == 0 {
        return 0;
    }

    // Round to nearest instead of truncating
    ((part as u128 * 100 + total as u128 / 2) / total as u128).min(i64::MAX as u128) as i64
}

/// Sum positive deltas of a monotonic counter across observations.
///
/// Returns `prev_total` unchanged on the first observation
/// (`last == 0`) and on counter resets (`current < last`), matching
/// the no-signal rule used by [`bytes_per_sec`]. Otherwise adds the
/// delta to the running total, saturating at `u64::MAX`.
pub(crate) fn accumulate_total(prev_total: u64, current: u64, last: u64) -> u64 {
    if last == 0 || current < last {
        return prev_total;
    }
    prev_total.saturating_add(current - last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_delta_normal() {
        assert_eq!(counter_delta(150, 100), CounterDelta::Delta(50));
    }

    #[test]
    fn counter_delta_reset() {
        assert_eq!(counter_delta(50, 100), CounterDelta::Reset);
    }

    #[test]
    fn bytes_per_sec_from_counter_delta() {
        assert_eq!(bytes_per_sec(3_000, 1_000, 2.0), 1_000);
        assert_eq!(bytes_per_sec(2_000, 1_000, 1.0), 1_000);
    }

    #[test]
    fn bytes_per_sec_zero_without_previous_or_elapsed() {
        assert_eq!(bytes_per_sec(1_000, 0, 1.0), 0);
        assert_eq!(bytes_per_sec(1_000, 500, 0.0), 0);
    }

    #[test]
    fn bytes_per_sec_zero_on_reset() {
        assert_eq!(bytes_per_sec(500, 1_000, 1.0), 0);
    }

    #[test]
    fn percent_u64_handles_zero_and_large_values() {
        assert_eq!(percent_u64(50, 100), 50);
        assert_eq!(percent_u64(1, 0), 0);
        assert_eq!(percent_u64(u64::MAX, u64::MAX), 100);
    }

    #[test]
    fn accumulate_total_returns_prev_on_first_observation() {
        // last == 0 means we haven't observed the counter yet this run.
        assert_eq!(accumulate_total(42, 1_000, 0), 42);
    }

    #[test]
    fn accumulate_total_returns_prev_on_counter_reset() {
        // current < last means the OS counter went backwards
        // (interface re-bind, etc.) — skip the delta.
        assert_eq!(accumulate_total(500, 100, 200), 500);
    }

    #[test]
    fn accumulate_total_adds_positive_delta() {
        assert_eq!(accumulate_total(100, 250, 200), 150);
    }

    #[test]
    fn accumulate_total_saturates_at_u64_max() {
        assert_eq!(accumulate_total(u64::MAX - 5, 100, 10), u64::MAX);
    }
}
