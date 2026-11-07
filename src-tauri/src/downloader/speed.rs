use std::collections::VecDeque;
use std::time::Instant;

/// Sliding-window + EWMA smoothed speed tracker.
/// Eliminates the sawtooth jitter and sudden dips to ~100 KB/s during
/// concurrent / multi-threaded batch downloads (e.g. modpacks, libraries, assets)
/// when threads briefly synchronize, hash files, or establish new connections.
#[derive(Debug, Clone)]
pub struct SpeedTracker {
    /// Window duration in seconds (default: 2.5s).
    window_duration_secs: f64,
    /// Ring buffer of (timestamp, cumulative_bytes) samples.
    samples: VecDeque<(Instant, u64)>,
    /// Smoothed speed in bytes/sec.
    smoothed_speed: f64,
    /// EWMA weighting factor (0.01 to 1.0).
    alpha: f64,
    /// How many consecutive update calls saw zero byte delta.
    consecutive_zero_count: u32,
}

impl Default for SpeedTracker {
    fn default() -> Self {
        Self::new(0)
    }
}

impl SpeedTracker {
    /// Creates a new SpeedTracker initialized with initial downloaded bytes.
    /// Default window duration is 2.5 seconds with alpha = 0.35.
    pub fn new(initial_bytes: u64) -> Self {
        Self::with_window_and_alpha(initial_bytes, 2.5, 0.35)
    }

    /// Creates a new SpeedTracker with a custom alpha weighting factor (0.01 to 1.0).
    pub fn with_alpha(initial_bytes: u64, alpha: f64) -> Self {
        Self::with_window_and_alpha(initial_bytes, 2.5, alpha)
    }

    /// Creates a new SpeedTracker with custom window duration and alpha.
    pub fn with_window_and_alpha(initial_bytes: u64, window_secs: f64, alpha: f64) -> Self {
        let now = Instant::now();
        let mut samples = VecDeque::with_capacity(16);
        samples.push_back((now, initial_bytes));
        Self {
            window_duration_secs: window_secs.max(0.5),
            samples,
            smoothed_speed: 0.0,
            alpha: alpha.clamp(0.01, 1.0),
            consecutive_zero_count: 0,
        }
    }

    /// Updates the tracker with the latest cumulative byte count and returns the smoothed speed in bytes/sec.
    pub fn update(&mut self, current_bytes: u64) -> u64 {
        let now = Instant::now();

        let last_sample_bytes = self.samples.back().map(|(_, b)| *b).unwrap_or(0);
        let bytes_diff_last = current_bytes.saturating_sub(last_sample_bytes);

        if bytes_diff_last == 0 {
            self.consecutive_zero_count += 1;
        } else {
            self.consecutive_zero_count = 0;
        }

        // Push current sample
        self.samples.push_back((now, current_bytes));

        // Prune samples that fall outside the sliding window
        // Keep at least the 2 most recent samples so we always have a duration
        while self.samples.len() > 2 {
            let oldest_time = self.samples.front().unwrap().0;
            if now.duration_since(oldest_time).as_secs_f64() > self.window_duration_secs {
                self.samples.pop_front();
            } else {
                break;
            }
        }

        let oldest = self.samples.front().unwrap();
        let elapsed = now.duration_since(oldest.0).as_secs_f64();

        if elapsed <= 0.0 {
            return self.smoothed_speed.round() as u64;
        }

        let window_bytes = current_bytes.saturating_sub(oldest.1);
        let raw_window_speed = (window_bytes as f64) / elapsed;

        if bytes_diff_last == 0 {
            // No bytes in this sampling interval:
            if self.consecutive_zero_count <= 2 {
                // Short handoff / transition gap (< 1.5s):
                // The sliding window's rolling throughput naturally bridges the gap.
                self.smoothed_speed = self.alpha * raw_window_speed + (1.0 - self.alpha) * self.smoothed_speed;
            } else if self.consecutive_zero_count <= 4 {
                // Extended pause (1.5s - 2.5s): decay smoothly
                self.smoothed_speed *= 0.5;
            } else {
                // Prolonged silence (>= 2.5s with zero bytes): settle to 0
                self.smoothed_speed = 0.0;
            }
        } else {
            if self.smoothed_speed <= 0.0 {
                // First non-zero measurement: initialize immediately
                self.smoothed_speed = raw_window_speed;
            } else {
                // Blend sliding-window throughput with EWMA for ultra-stable speed display
                self.smoothed_speed = self.alpha * raw_window_speed + (1.0 - self.alpha) * self.smoothed_speed;
            }
        }

        if self.smoothed_speed < 256.0 {
            self.smoothed_speed = 0.0;
        }

        self.smoothed_speed.round() as u64
    }

    /// Explicitly resets the smoothed speed and sliding window.
    pub fn reset(&mut self, current_bytes: u64) {
        let now = Instant::now();
        self.samples.clear();
        self.samples.push_back((now, current_bytes));
        self.smoothed_speed = 0.0;
        self.consecutive_zero_count = 0;
    }

    /// Returns current smoothed speed in bytes/sec.
    pub fn current_speed(&self) -> u64 {
        self.smoothed_speed.round() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_initial_speed_jump() {
        let mut tracker = SpeedTracker::new(0);
        // Initial sample at t = 0 (0 bytes)
        let t0 = Instant::now() - Duration::from_millis(500);
        tracker.samples.clear();
        tracker.samples.push_back((t0, 0));

        let speed = tracker.update(1024 * 1024); // 1 MB in 0.5s => 2 MB/s
        assert!((1_900_000..=2_200_000).contains(&speed), "Initial speed should be ~2 MB/s: got {}", speed);
    }

    #[test]
    fn test_parallel_download_burst_and_trough() {
        // Models parallel download of mod files:
        // Workers streaming: 5MB in 0.5s (10 MB/s)
        // Workers streaming: 5MB in 0.5s (10 MB/s)
        // Handoff trough: only 50KB in 0.5s (100 KB/s instant delta)
        // Workers resume: 5MB in 0.5s (10 MB/s)
        let mut tracker = SpeedTracker::new(0);
        let t0 = Instant::now() - Duration::from_millis(2000);
        tracker.samples.clear();
        tracker.samples.push_back((t0, 0));

        // t = 0.5s: +5 MB (total 5 MB)
        let t1 = t0 + Duration::from_millis(500);
        tracker.samples.push_back((t1, 5_000_000));
        tracker.smoothed_speed = 10_000_000.0;

        // t = 1.0s: +5 MB (total 10 MB)
        let t2 = t0 + Duration::from_millis(1000);
        tracker.samples.push_back((t2, 10_000_000));
        tracker.smoothed_speed = 10_000_000.0;

        // t = 1.5s: Handoff gap! Only +50 KB arrived (instant speed would be 100 KB/s!)
        // Simulate update at t3 = t0 + 1500ms
        let t3 = t0 + Duration::from_millis(1500);
        // Set tracker state right before update at t3
        // We will call update by temporarily shifting samples
        tracker.samples.clear();
        tracker.samples.push_back((t0, 0));
        tracker.samples.push_back((t1, 5_000_000));
        tracker.samples.push_back((t2, 10_000_000));
        // Prune logic inside update will run against Instant::now() if not shifted,
        // so let's shift relative to Instant::now():
        let now = Instant::now();
        tracker.samples.clear();
        tracker.samples.push_back((now - Duration::from_millis(1500), 0));
        tracker.samples.push_back((now - Duration::from_millis(1000), 5_000_000));
        tracker.samples.push_back((now - Duration::from_millis(500), 10_000_000));
        tracker.smoothed_speed = 10_000_000.0;

        // At this moment (now), current_bytes is 10_050_000 (only 50KB added in last 500ms)
        let trough_speed = tracker.update(10_050_000);

        // The speed MUST NOT drop to 100 KB/s! It should stay above 5 MB/s
        assert!(
            trough_speed > 5_000_000,
            "Speed during handoff trough should be maintained above 5 MB/s: got {}",
            trough_speed
        );
        assert!(
            trough_speed <= 10_000_000,
            "Speed should not exceed max: got {}",
            trough_speed
        );
    }

    #[test]
    fn test_zero_bytes_graceful_decay() {
        let mut tracker = SpeedTracker::new(0);
        let now = Instant::now();
        tracker.samples.clear();
        tracker.samples.push_back((now - Duration::from_millis(500), 0));
        let initial_speed = tracker.update(5_000_000); // 10 MB/s
        assert!(initial_speed > 0);

        // First zero tick: file boundary / TCP pause
        let mut decay_speed = initial_speed;
        for i in 1..=6 {
            let fake_now = now + Duration::from_millis(i * 500);
            // Replace newest sample time
            if let Some(back) = tracker.samples.back_mut() {
                back.0 = fake_now - Duration::from_millis(500);
            }
            decay_speed = tracker.update(5_000_000);
        }

        // After >= 2.5s of consecutive zero bytes, it must settle cleanly to 0
        assert_eq!(decay_speed, 0, "Prolonged zero should settle cleanly to 0");
    }
}
