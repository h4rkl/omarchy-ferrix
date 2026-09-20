//! Lifecycle policy that is pure and clock-driven: the home surface restart
//! budget. State transitions themselves live in `ferrix_app_api::LifecycleState`.
//!
//! Only the home surface is restarted; other applications are never
//! auto-restarted. Delays grow per unrequested exit inside a sliding window
//! (immediately, then 1 s, 2 s, 4 s); when the window holds `max_exits`
//! exits the service gives up until an explicit `Home`/`Launch` request.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeRestartConfig {
    /// Sliding window over which exits are counted.
    pub window: Duration,
    /// Exits inside the window that abandon restarts (inclusive).
    pub max_exits: usize,
    /// Delay before the n-th restart; the last entry repeats.
    pub delays: Vec<Duration>,
}

impl Default for HomeRestartConfig {
    fn default() -> Self {
        Self {
            window: Duration::from_secs(60),
            max_exits: 5,
            delays: vec![
                Duration::ZERO,
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
            ],
        }
    }
}

impl HomeRestartConfig {
    /// Backoff for restart `attempt` (1-based); saturates at the last delay.
    pub fn delay_for(&self, attempt: u32) -> Duration {
        let index = attempt.saturating_sub(1) as usize;
        self.delays
            .get(index)
            .or(self.delays.last())
            .copied()
            .unwrap_or(Duration::ZERO)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartDecision {
    Restart { attempt: u32, delay: Duration },
    Abandon { exits: usize },
}

/// Exit history of the home surface.
#[derive(Clone, Debug, Default)]
pub struct HomeRestartTracker {
    exits: VecDeque<Instant>,
    abandoned: bool,
}

impl HomeRestartTracker {
    /// Records an unrequested exit at `now` and decides what to do.
    pub fn record_exit(&mut self, config: &HomeRestartConfig, now: Instant) -> RestartDecision {
        while self
            .exits
            .front()
            .is_some_and(|first| now.duration_since(*first) > config.window)
        {
            self.exits.pop_front();
        }
        self.exits.push_back(now);
        let exits = self.exits.len();
        if exits >= config.max_exits {
            self.abandoned = true;
            return RestartDecision::Abandon { exits };
        }
        let attempt = exits as u32;
        RestartDecision::Restart {
            attempt,
            delay: config.delay_for(attempt),
        }
    }

    /// Forgets the history: an explicit launch starts a fresh budget.
    pub fn reset(&mut self) {
        self.exits.clear();
        self.abandoned = false;
    }

    pub fn abandoned(&self) -> bool {
        self.abandoned
    }

    pub fn recent_exits(&self) -> usize {
        self.exits.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_follow_the_documented_schedule_and_saturate() {
        let config = HomeRestartConfig::default();
        assert_eq!(config.delay_for(1), Duration::ZERO);
        assert_eq!(config.delay_for(2), Duration::from_secs(1));
        assert_eq!(config.delay_for(3), Duration::from_secs(2));
        assert_eq!(config.delay_for(4), Duration::from_secs(4));
        assert_eq!(config.delay_for(9), Duration::from_secs(4));
    }

    #[test]
    fn gives_up_after_five_exits_in_the_window() {
        let config = HomeRestartConfig::default();
        let mut tracker = HomeRestartTracker::default();
        let base = Instant::now();
        let mut decisions = Vec::new();
        for i in 0..5u64 {
            decisions.push(tracker.record_exit(&config, base + Duration::from_secs(i)));
        }
        assert_eq!(
            decisions[0],
            RestartDecision::Restart {
                attempt: 1,
                delay: Duration::ZERO
            }
        );
        assert_eq!(
            decisions[3],
            RestartDecision::Restart {
                attempt: 4,
                delay: Duration::from_secs(4)
            }
        );
        assert_eq!(decisions[4], RestartDecision::Abandon { exits: 5 });
        assert!(tracker.abandoned());
    }

    #[test]
    fn old_exits_fall_out_of_the_window() {
        let config = HomeRestartConfig::default();
        let mut tracker = HomeRestartTracker::default();
        let base = Instant::now();
        for i in 0..4u64 {
            tracker.record_exit(&config, base + Duration::from_secs(i));
        }
        assert_eq!(tracker.recent_exits(), 4);
        // Two minutes later the history is stale: back to an immediate restart.
        let decision = tracker.record_exit(&config, base + Duration::from_secs(120));
        assert_eq!(
            decision,
            RestartDecision::Restart {
                attempt: 1,
                delay: Duration::ZERO
            }
        );
        tracker.reset();
        assert_eq!(tracker.recent_exits(), 0);
        assert!(!tracker.abandoned());
    }
}
