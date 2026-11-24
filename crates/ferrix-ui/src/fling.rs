//! Touch fling for one-axis scrolling lists: a release-velocity estimator
//! and an exponentially decelerating glide. Pure and time-injected: sample
//! times are input-event milliseconds (the Wayland event clock), and the
//! glide runs on whatever [`Instant`] the caller passes, so tests drive both
//! deterministically. No Wayland, no rendering.
//!
//! Velocity is a least-squares slope over the samples in the last
//! [`HORIZON_MS`] before release, which tolerates jittery 120 Hz panels and
//! batched delivery. Sparse input (QEMU's tablet sends a motion every ~20 ms
//! or less often) falls back to the last two samples when the window holds
//! fewer than two. A finger that rested for [`STALL_MS`] before lifting
//! releases with zero velocity, so a deliberate stop never glides.
//!
//! The glide follows `v(t) = v0 * exp(-t / TAU_MS)`, so the offset settles at
//! `v0 * TAU_MS`; it ends once the speed drops below [`STOP_VELOCITY`]. The
//! caller clamps the position to its content and drops the fling at an end
//! (no overscroll bounce).

use std::time::{Duration, Instant};

/// Window of samples, before the release, that the velocity fit uses.
pub const HORIZON_MS: f64 = 100.0;
/// A finger still for this long before lifting releases without a fling.
pub const STALL_MS: f64 = 50.0;
/// Oldest sample pair the sparse fallback still trusts.
pub const SPARSE_GAP_MS: f64 = 250.0;
/// Glide time constant (the classic kinetic-scrolling value).
pub const TAU_MS: f64 = 325.0;
/// Speed (px/ms) at which a glide ends: under a pixel per 60 Hz frame.
pub const STOP_VELOCITY: f64 = 0.05;
/// Glide frame period: the timer interval while a fling moves.
pub const FRAME: Duration = Duration::from_millis(16);

const MAX_SAMPLES: usize = 16;

/// Recent `(time_ms, position)` samples of one contact along one axis.
#[derive(Clone, Debug, Default)]
pub struct VelocityTracker {
    samples: Vec<(f64, f64)>,
}

impl VelocityTracker {
    pub fn reset(&mut self) {
        self.samples.clear();
    }

    /// Records `position` at `time_ms`. A sample at the same (or an earlier,
    /// reordered) time replaces the newest one instead of adding a zero-length
    /// interval.
    pub fn add(&mut self, time_ms: f64, position: f64) {
        if let Some(last) = self.samples.last_mut()
            && time_ms <= last.0
        {
            last.1 = position;
            return;
        }
        if self.samples.len() == MAX_SAMPLES {
            self.samples.remove(0);
        }
        self.samples.push((time_ms, position));
    }

    /// Velocity in position units per millisecond at a release at
    /// `release_ms`; zero when the contact stalled or has too few samples.
    pub fn velocity(&self, release_ms: f64) -> f64 {
        let Some(&(last_t, _)) = self.samples.last() else {
            return 0.0;
        };
        if release_ms - last_t > STALL_MS {
            return 0.0;
        }
        let recent: Vec<(f64, f64)> = self
            .samples
            .iter()
            .copied()
            .filter(|(t, _)| last_t - t <= HORIZON_MS)
            .collect();
        if recent.len() >= 2 {
            return slope(&recent);
        }
        // Sparse input: the newest pair, when not too far apart.
        match self.samples.as_slice() {
            [.., a, b] if b.0 - a.0 <= SPARSE_GAP_MS => (b.1 - a.1) / (b.0 - a.0),
            _ => 0.0,
        }
    }
}

/// Least-squares slope of `samples` (at least two distinct times).
fn slope(samples: &[(f64, f64)]) -> f64 {
    let n = samples.len() as f64;
    let mean_t = samples.iter().map(|s| s.0).sum::<f64>() / n;
    let mean_p = samples.iter().map(|s| s.1).sum::<f64>() / n;
    let (mut num, mut den) = (0.0, 0.0);
    for (t, p) in samples {
        num += (t - mean_t) * (p - mean_p);
        den += (t - mean_t) * (t - mean_t);
    }
    if den > 0.0 { num / den } else { 0.0 }
}

/// A decelerating glide from `origin` at `velocity` (units per ms).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fling {
    origin: f64,
    velocity: f64,
    start: Instant,
    /// When the next frame is due.
    next: Instant,
}

impl Fling {
    /// A glide starting `now`, or `None` when `velocity` is below `min`.
    /// `velocity` is clamped to `±max`.
    pub fn start(origin: f64, velocity: f64, min: f64, max: f64, now: Instant) -> Option<Self> {
        if !velocity.is_finite() || velocity.abs() < min.max(STOP_VELOCITY) {
            return None;
        }
        Some(Self {
            origin,
            velocity: velocity.clamp(-max, max),
            start: now,
            next: now + FRAME,
        })
    }

    fn elapsed_ms(&self, now: Instant) -> f64 {
        now.saturating_duration_since(self.start).as_secs_f64() * 1000.0
    }

    /// Position at `now`.
    pub fn position(&self, now: Instant) -> f64 {
        let decay = (-self.elapsed_ms(now) / TAU_MS).exp();
        self.origin + self.velocity * TAU_MS * (1.0 - decay)
    }

    /// Speed at `now`, in units per ms.
    pub fn velocity(&self, now: Instant) -> f64 {
        self.velocity * (-self.elapsed_ms(now) / TAU_MS).exp()
    }

    /// Speed at the start (after clamping), in units per ms.
    pub fn initial_velocity(&self) -> f64 {
        self.velocity
    }

    /// The glide has slowed below [`STOP_VELOCITY`] by `now`.
    pub fn finished(&self, now: Instant) -> bool {
        self.velocity(now).abs() < STOP_VELOCITY
    }

    /// When the caller should next advance the glide.
    pub fn deadline(&self) -> Instant {
        self.next
    }

    /// Advances to `now`: `Some(position)` while gliding (schedules the next
    /// frame), `None` once finished (the caller drops the fling).
    pub fn advance(&mut self, now: Instant) -> Option<f64> {
        if self.finished(now) {
            return None;
        }
        self.next = now + FRAME;
        Some(self.position(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn steady_120hz_drag_estimates_its_speed() {
        let mut tracker = VelocityTracker::default();
        // 2 px/ms upward (negative), one sample every 8 ms.
        for i in 0..20 {
            let t = f64::from(i) * 8.0;
            tracker.add(t, 1000.0 - 2.0 * t);
        }
        let v = tracker.velocity(152.0 + 4.0);
        assert!((v + 2.0).abs() < 1e-9, "{v}");
    }

    #[test]
    fn the_fit_uses_only_the_recent_window() {
        let mut tracker = VelocityTracker::default();
        // Slow for 200 ms, then fast for the last 80 ms.
        for i in 0..=25 {
            tracker.add(f64::from(i) * 8.0, f64::from(i) * 8.0 * 0.1);
        }
        let base = 200.0 * 0.1;
        for i in 1..=10 {
            let t = 200.0 + f64::from(i) * 8.0;
            tracker.add(t, base + (t - 200.0) * 3.0);
        }
        let v = tracker.velocity(280.0);
        assert!(v > 2.0, "recent speed dominates: {v}");
    }

    #[test]
    fn jitter_and_duplicate_times_do_not_spike() {
        let mut tracker = VelocityTracker::default();
        for i in 0..12 {
            let t = f64::from(i) * 8.0;
            let jitter = if i % 2 == 0 { 3.0 } else { -3.0 };
            tracker.add(t, t * 1.5 + jitter);
            // A batched duplicate at the same millisecond.
            tracker.add(t, t * 1.5 + jitter);
        }
        let v = tracker.velocity(90.0);
        assert!((v - 1.5).abs() < 0.2, "{v}");
    }

    #[test]
    fn sparse_input_uses_the_last_pair() {
        let mut tracker = VelocityTracker::default();
        tracker.add(0.0, 0.0);
        tracker.add(150.0, 300.0);
        assert!((tracker.velocity(160.0) - 2.0).abs() < 1e-9);
        // Too far apart to trust.
        let mut tracker = VelocityTracker::default();
        tracker.add(0.0, 0.0);
        tracker.add(400.0, 300.0);
        assert_eq!(tracker.velocity(400.0), 0.0);
        // A single sample has no velocity.
        let mut tracker = VelocityTracker::default();
        tracker.add(0.0, 10.0);
        assert_eq!(tracker.velocity(5.0), 0.0);
        assert_eq!(VelocityTracker::default().velocity(0.0), 0.0);
    }

    #[test]
    fn a_finger_that_rests_before_lifting_does_not_fling() {
        let mut tracker = VelocityTracker::default();
        for i in 0..10 {
            tracker.add(f64::from(i) * 8.0, f64::from(i) * 16.0);
        }
        assert!(tracker.velocity(72.0 + STALL_MS) > 1.0);
        assert_eq!(tracker.velocity(72.0 + STALL_MS + 1.0), 0.0);
        tracker.reset();
        assert_eq!(tracker.velocity(0.0), 0.0);
    }

    #[test]
    fn a_glide_decelerates_monotonically_and_settles() {
        let t0 = Instant::now();
        let mut fling = Fling::start(100.0, 4.0, 0.3, 10.0, t0).unwrap();
        assert_eq!(fling.deadline(), t0 + FRAME);
        let mut last = 100.0;
        let mut step = f64::INFINITY;
        let mut ms: u32 = 0;
        loop {
            ms += 16;
            let Some(p) = fling.advance(at(t0, u64::from(ms))) else {
                break;
            };
            assert!(p > last, "moves forward");
            assert!(p - last < step, "each frame moves less");
            step = p - last;
            last = p;
            assert_eq!(fling.deadline(), at(t0, u64::from(ms) + 16));
            assert!(ms < 5000, "terminates");
        }
        // Settles near v0 * tau without overshooting it.
        let total = 4.0 * TAU_MS;
        assert!(
            last < 100.0 + total && last > 100.0 + total - 25.0,
            "{last}"
        );
        // Duration: v0 * exp(-t/tau) < STOP  =>  t = tau * ln(v0 / STOP).
        let expected = TAU_MS * (4.0 / STOP_VELOCITY).ln();
        assert!(
            (f64::from(ms) - expected).abs() <= 16.0,
            "{ms} vs {expected}"
        );
    }

    #[test]
    fn slow_releases_do_not_glide_and_fast_ones_clamp() {
        let t0 = Instant::now();
        assert!(Fling::start(0.0, 0.2, 0.3, 10.0, t0).is_none());
        assert!(Fling::start(0.0, f64::NAN, 0.3, 10.0, t0).is_none());
        let fling = Fling::start(0.0, -50.0, 0.3, 10.0, t0).unwrap();
        assert_eq!(fling.velocity(t0), -10.0);
        assert!(fling.position(at(t0, 100)) < 0.0, "negative glides go back");
    }
}
