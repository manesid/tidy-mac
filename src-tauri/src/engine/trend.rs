//! Pure trend math over usage samples: "at this rate the disk is full in ~N days".

use super::store::Sample;
use serde::Serialize;

#[derive(Serialize, Debug, Clone, Copy, PartialEq)]
pub struct Forecast {
    /// Bytes per day (positive = filling up).
    pub bytes_per_day: f64,
    /// Days until free space reaches zero, if usage is growing.
    pub days_to_full: Option<f64>,
    /// Span of data the estimate is based on, in days.
    pub span_days: f64,
}

/// Least-squares slope over the samples in the last `window` seconds.
/// Needs at least `min_span` seconds of data and 3 samples, otherwise returns None (not enough evidence).
pub fn forecast(samples: &[Sample], now: u64, window: u64, min_span: u64) -> Option<Forecast> {
    let pts: Vec<&Sample> = samples.iter().filter(|s| s.ts + window >= now && s.ts <= now).collect();
    if pts.len() < 3 {
        return None;
    }
    let (t0, t1) = (pts.first()?.ts, pts.last()?.ts);
    if t1.saturating_sub(t0) < min_span {
        return None;
    }
    let n = pts.len() as f64;
    let xs: Vec<f64> = pts.iter().map(|s| (s.ts - t0) as f64 / 86_400.0).collect();
    let ys: Vec<f64> = pts.iter().map(|s| s.used as f64).collect();
    let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    if sxx == 0.0 {
        return None;
    }
    let slope = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum::<f64>() / sxx;
    let last = pts.last()?;
    let free = last.total.saturating_sub(last.used) as f64;
    // Only forecast meaningful growth (> 50 MB/day); noise otherwise.
    let days_to_full = (slope > 50e6).then(|| free / slope);
    Some(Forecast { bytes_per_day: slope, days_to_full, span_days: (t1 - t0) as f64 / 86_400.0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    const D: u64 = 86_400;
    const GB: u64 = 1_000_000_000;

    fn series(days: u64, start_used: u64, per_day: i64) -> Vec<Sample> {
        (0..=days * 4).map(|i| Sample { ts: 100 * D + i * D / 4, used: (start_used as i64 + per_day * i as i64 / 4) as u64, total: 500 * GB }).collect()
    }

    #[test]
    fn linear_growth_predicts_days_to_full() {
        let s = series(10, 400 * GB, GB as i64); // +1 GB/day, ends at 410 GB used → 90 GB free
        let f = forecast(&s, 110 * D, 14 * D, 2 * D).unwrap();
        assert!((f.bytes_per_day - GB as f64).abs() < 1e6);
        assert!((f.days_to_full.unwrap() - 90.0).abs() < 0.5);
    }
    #[test]
    fn shrinking_or_flat_usage_has_no_eta() {
        let s = series(10, 400 * GB, -(GB as i64));
        assert_eq!(forecast(&s, 110 * D, 14 * D, 2 * D).unwrap().days_to_full, None);
    }
    #[test]
    fn needs_enough_history() {
        let s = series(1, 400 * GB, GB as i64);
        assert!(forecast(&s, 101 * D, 14 * D, 2 * D).is_none());
        assert!(forecast(&[], 0, 14 * D, 2 * D).is_none());
    }
}
