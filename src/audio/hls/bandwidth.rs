//! Bandwidth estimation + adaptive bitrate (ABR) tier selection (Phase 5 of HLS playback).
//!
//! Implements the RFC 8216 §7.2.1 download-rate estimation approach:
//! - Each downloaded segment contributes a sample `(bytes, seconds)` → `bw = bytes·8 / seconds`.
//! - The estimate is the **harmonic mean** of the last `N` segment bandwidths (RFC 8216
//!   recommends the harmonic mean, which is dominated by the slowest segment — the safe
//!   choice for live streaming).
//! - Tier selection picks the **highest** variant whose `BANDWIDTH` fits under
//!   `estimate · safety`. Up-switching requires the estimate to clear the next tier for
//!   `UP_STREAK` consecutive segments (hysteresis); down-switching is immediate.

use std::collections::VecDeque;

/// Number of recent segments kept in the sliding window.
const WINDOW_SIZE: usize = 3;

/// Consecutive segments above the next tier's ceiling required before switching up.
const UP_STREAK: usize = 2;

/// Safety factor applied to the estimated bandwidth when choosing a tier.
const SAFETY: f64 = 0.9;

/// Sliding-window bandwidth estimator (harmonic mean of recent segment download rates).
pub struct BandwidthEstimator {
    window: VecDeque<(u64, f64)>,
    window_size: usize,
}

impl BandwidthEstimator {
    pub fn new() -> Self {
        Self {
            window: VecDeque::new(),
            window_size: WINDOW_SIZE,
        }
    }

    /// Records one segment download: `bytes` fetched in `secs` seconds.
    pub fn record(&mut self, bytes: u64, secs: f64) {
        if bytes == 0 || secs <= 0.0 {
            return;
        }
        self.window.push_back((bytes, secs));
        while self.window.len() > self.window_size {
            self.window.pop_front();
        }
    }

    /// The current estimate in bits per second, or `None` if no samples yet.
    pub fn estimate(&self) -> Option<u64> {
        if self.window.is_empty() {
            return None;
        }
        let mut sum_inv = 0.0f64;
        let mut count = 0usize;
        for &(bytes, secs) in &self.window {
            let bw = (bytes as f64) * 8.0 / secs;
            if bw > 0.0 {
                sum_inv += 1.0 / bw;
                count += 1;
            }
        }
        if count == 0 || sum_inv <= 0.0 {
            return None;
        }
        Some((count as f64 / sum_inv).round() as u64)
    }
}

/// Adaptive tier selector with hysteresis. Tracks the current tier index (into a
/// bandwidth-ascending variant list) and the number of consecutive segments whose
/// estimate cleared the next tier.
pub struct TierSelector {
    current: usize,
    up_streak: usize,
}

impl TierSelector {
    pub fn new(start: usize) -> Self {
        Self {
            current: start,
            up_streak: 0,
        }
    }

    pub fn current(&self) -> usize {
        self.current
    }

    /// Chooses the target tier given the current estimate. `bandwidths` must be sorted
    /// ascending. Returns the new current tier index.
    pub fn select(&mut self, est_bw: u64, bandwidths: &[u64]) -> usize {
        if bandwidths.is_empty() {
            return self.current;
        }
        let ceiling = (est_bw as f64 * SAFETY) as u64;
        // Highest tier whose bandwidth fits under the ceiling.
        let mut target = 0usize;
        for (i, &bw) in bandwidths.iter().enumerate() {
            if bw <= ceiling {
                target = i;
            }
        }

        if target > self.current {
            self.up_streak += 1;
            if self.up_streak >= UP_STREAK {
                self.current = target;
                self.up_streak = 0;
            }
        } else {
            // Down-switch (or stay) is immediate.
            self.up_streak = 0;
            self.current = target;
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
use super::*;

    #[test]
    fn estimator_harmonic_mean() {
        let mut e = BandwidthEstimator::new();
        // 100_000 bytes in 1s = 800_000 bps; 100_000 bytes in 2s = 400_000 bps.
        e.record(100_000, 1.0);
        e.record(100_000, 2.0);
        // Harmonic mean of 800k and 400k = 2 / (1/800k + 1/400k) = 533_333.
        let est = e.estimate().unwrap();
        assert!((est - 533_333) < 1000, "got {est}");
    }

    #[test]
    fn estimator_window_caps_at_three() {
        let mut e = BandwidthEstimator::new();
        for _ in 0..10 {
            e.record(100_000, 1.0);
        }
        assert_eq!(e.window.len(), 3);
    }

    #[test]
    fn tier_starts_low_and_climbs_with_hysteresis() {
        let bandwidths = [52_800u64, 105_600, 211_200];
        let mut sel = TierSelector::new(0);
        // Estimate well above hifi: needs 2 consecutive segments to climb.
        assert_eq!(sel.select(300_000, &bandwidths), 0); // streak 1, stays
        assert_eq!(sel.select(300_000, &bandwidths), 2); // streak 2, jumps to hifi
    }

    #[test]
    fn tier_drops_immediately() {
        let bandwidths = [52_800u64, 105_600, 211_200];
        let mut sel = TierSelector::new(2);
        // Estimate below lofi ceiling → drop to lofi at once.
        assert_eq!(sel.select(40_000, &bandwidths), 0);
    }

    #[test]
    fn tier_respects_safety_margin() {
        let bandwidths = [52_800u64, 105_600, 211_200];
        let mut sel = TierSelector::new(0);
        // 120_000 * 0.9 = 108_000 → fits midfi (105_600) but not hifi (211_200).
        sel.select(120_000, &bandwidths);
        assert_eq!(sel.select(120_000, &bandwidths), 1);
    }
}
