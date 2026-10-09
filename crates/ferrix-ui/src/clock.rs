//! Local wall clock for every status bar.
//!
//! Surfaces show `HH:MM` in the session's local time zone and redraw at each
//! minute boundary. [`MinuteClock`] holds the minute a surface last drew;
//! [`spawn_minute_ticker`] calls a wake handle just after every boundary so
//! the surface re-reads the clock and redraws only when the minute moved.
//!
//! Time-zone resolution uses `jiff` (BurntSushi, MIT OR Unlicense): it honours
//! `TZ` (POSIX rules or IANA names looked up in the system zoneinfo) and then
//! `/etc/localtime`, re-checking the system zone as it changes, and falls
//! back to UTC when neither exists (the minimal QEMU guest ships no tzdata).
//! It is pure Rust, so the workspace's `unsafe_code = "forbid"` rules out
//! calling `localtime_r` directly, and hand-rolling a TZif/POSIX-TZ parser
//! would duplicate a maintained library.

use std::thread;
use std::time::Duration;

use jiff::Timestamp;
use jiff::tz::TimeZone;

/// Minutes in a day; [`MinuteClock::minute`] is always below this.
pub const MINUTES_PER_DAY: u32 = 24 * 60;

/// Slack after the boundary so the ticker never wakes a hair early and reads
/// the previous minute again.
const BOUNDARY_SLACK: Duration = Duration::from_millis(50);

/// Minute of the day (`0..1440`) of `timestamp` in `zone`.
pub fn minute_of_day_in(timestamp: Timestamp, zone: &TimeZone) -> u32 {
    let civil = zone.to_datetime(timestamp);
    // jiff guarantees hour 0..=23 and minute 0..=59.
    u32::from(civil.hour().unsigned_abs()) * 60 + u32::from(civil.minute().unsigned_abs())
}

/// The current minute of the day in the session's local time zone.
pub fn local_minute_of_day() -> u32 {
    minute_of_day_in(Timestamp::now(), &TimeZone::system())
}

/// `HH:MM` for a minute of the day.
pub fn hhmm(minute_of_day: u32) -> String {
    let minute = minute_of_day % MINUTES_PER_DAY;
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// Time from `timestamp` until just after the next whole UTC minute. Every
/// zone in use today has a whole-minute offset, so this is also the next
/// local minute boundary.
pub fn until_next_minute_from(timestamp: Timestamp) -> Duration {
    let into_minute = timestamp.as_millisecond().rem_euclid(60_000);
    Duration::from_millis((60_000 - into_minute).unsigned_abs()) + BOUNDARY_SLACK
}

/// Time from now until just after the next minute boundary.
pub fn until_next_minute() -> Duration {
    until_next_minute_from(Timestamp::now())
}

/// The minute a surface last drew, refreshed on wake.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MinuteClock {
    minute: u32,
}

impl MinuteClock {
    /// A clock reading the current local minute.
    pub fn now() -> Self {
        Self {
            minute: local_minute_of_day(),
        }
    }

    /// The minute of the day to draw.
    pub fn minute(&self) -> u32 {
        self.minute
    }

    /// Re-reads the local minute; true when it changed and a redraw is due.
    pub fn refresh(&mut self) -> bool {
        self.set(local_minute_of_day())
    }

    /// Stores `minute`; true when it differs from the last one.
    pub fn set(&mut self, minute: u32) -> bool {
        let changed = minute != self.minute;
        self.minute = minute;
        changed
    }
}

/// Spawns a detached thread that calls `notify` just after every minute
/// boundary for the life of the process. Pass a surface's wake handle; its
/// wake path then calls [`MinuteClock::refresh`].
pub fn spawn_minute_ticker<F>(notify: F)
where
    F: Fn() + Send + 'static,
{
    thread::Builder::new()
        .name("ferrix-clock".to_owned())
        .spawn(move || {
            loop {
                thread::sleep(until_next_minute());
                notify();
            }
        })
        .expect("spawn ferrix-clock ticker thread");
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::tz;

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).expect("valid timestamp")
    }

    #[test]
    fn minute_of_day_follows_the_zone_offset() {
        // 2026-10-09T23:30:00Z.
        let ts = at(1_791_588_600);
        assert_eq!(minute_of_day_in(ts, &TimeZone::UTC), 23 * 60 + 30);
        assert_eq!(
            minute_of_day_in(ts, &TimeZone::fixed(tz::offset(10))),
            9 * 60 + 30
        );
        assert_eq!(
            minute_of_day_in(ts, &TimeZone::fixed(tz::offset(-5))),
            18 * 60 + 30
        );
    }

    #[test]
    fn minute_of_day_honours_posix_tz_rules() {
        // Sydney is on daylight time (UTC+11) in October.
        let sydney = TimeZone::posix("AEST-10AEDT,M10.1.0,M4.1.0/3").expect("posix tz");
        assert_eq!(minute_of_day_in(at(1_791_588_600), &sydney), 10 * 60 + 30);
    }

    #[test]
    fn formats_hours_and_minutes() {
        assert_eq!(hhmm(0), "00:00");
        assert_eq!(hhmm(9 * 60 + 5), "09:05");
        assert_eq!(hhmm(23 * 60 + 59), "23:59");
        assert_eq!(hhmm(MINUTES_PER_DAY + 1), "00:01");
    }

    #[test]
    fn next_minute_delay_lands_just_after_the_boundary() {
        let ts = at(1_791_588_600);
        assert_eq!(
            until_next_minute_from(ts),
            Duration::from_secs(60) + BOUNDARY_SLACK
        );
        let late = ts
            .checked_add(Duration::from_millis(59_900))
            .expect("in range");
        assert_eq!(
            until_next_minute_from(late),
            Duration::from_millis(100) + BOUNDARY_SLACK
        );
    }

    #[test]
    fn refresh_reports_only_changes() {
        let mut clock = MinuteClock { minute: 10 };
        assert!(!clock.set(10));
        assert!(clock.set(11));
        assert_eq!(clock.minute(), 11);
        assert!(local_minute_of_day() < MINUTES_PER_DAY);
    }
}
