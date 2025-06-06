// Oprs -- process monitor for Linux
// Copyright (C) 2020-2025  Laurent Pelecq
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use libc::{nanosleep, timespec};
use std::time::{Duration, Instant};

/// Timer that expired at constant time
///
/// The stop watch records the time when the timer was started. It's used to
/// correct the remaining time.
pub struct Timer {
    delay: Duration,
    stop_watch: Instant,
    remaining: Option<Duration>,
}

impl Timer {
    /// Create a new timer already expired if second parameter is true.
    pub fn new(delay: Duration, expired: bool) -> Timer {
        Timer {
            delay,
            stop_watch: Instant::now(),
            remaining: if expired { None } else { Some(delay) },
        }
    }

    /// Delay before the timer expires.
    pub fn get_delay(&self) -> Duration {
        self.delay
    }

    /// Start time
    pub fn start_time(&self) -> Instant {
        self.stop_watch
    }

    /// change the delay of the timer. If it hasn't already expired, ajust the remaining time.
    #[cfg(feature = "tui")]
    pub fn set_delay(&mut self, delay: Duration) {
        if let Some(remaining) = self.remaining {
            self.remaining = if delay >= self.delay {
                // Delay increased: add the difference to the remaining time
                remaining.checked_add(delay.checked_sub(self.delay).unwrap())
            } else {
                // Delay decreased: reduce remaining time or expire
                remaining.checked_sub(self.delay.checked_sub(delay).unwrap())
            };
        }
        self.delay = delay;
    }

    /// Check if timer has expired.
    pub fn expired(&mut self) -> bool {
        self.remaining().is_none()
    }

    /// Reset the timer.
    ///
    /// The timer reference is not the current time but the last time it actually expired.
    pub fn reset(&mut self) {
        self.remaining = Some(self.delay);
    }

    /// Return the remaining time or None if it has expired.
    pub fn remaining(&mut self) -> Option<Duration> {
        if let Some(remaining) = self.remaining {
            let elapsed = self.stop_watch.elapsed();
            let now = Instant::now();
            if remaining == elapsed {
                self.remaining = None;
                self.stop_watch = now;
            } else {
                match remaining.checked_sub(elapsed) {
                    Some(remaining) => {
                        // elapsed time is less than remaining time.
                        self.remaining = Some(remaining);
                        self.stop_watch = now;
                    }
                    None => {
                        // elapsed time is greather than remaining time.
                        self.remaining = None;
                        // The start time for the timer is exactly when it expired.
                        self.stop_watch = now
                            .checked_sub(elapsed.checked_sub(remaining).unwrap())
                            .unwrap_or(now);
                    }
                }
            }
        }
        self.remaining
    }

    /// Check if the given delay has been reached
    ///
    /// Calls nanosleep to have an interruption on Ctrl-C
    pub fn sleep(&mut self, delay: Duration) -> Option<Duration> {
        let req = timespec {
            tv_sec: delay.as_secs() as i64,
            tv_nsec: delay.subsec_nanos() as i64,
        };
        let mut rem = timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let ret = unsafe { nanosleep(&req, &mut rem) };
        if ret == 0 || rem.tv_sec < 0 {
            None
        } else {
            let remaining = Duration::new(rem.tv_sec as u64, rem.tv_nsec as u32);
            Some(remaining)
        }
    }
}

/// Report difference between an expected elapsed time and the actual elapsed time
pub struct DriftMonitor {
    start_time: Instant,
    notification_time: Instant,
    notification_delay: u64,
    expected_elapsed: Duration,
}

impl DriftMonitor {
    /// New monitor with a start time and a delay between two notifications.
    pub fn new(start_time: Instant, notification_delay: u64) -> DriftMonitor {
        DriftMonitor {
            start_time,
            notification_time: start_time,
            notification_delay,
            expected_elapsed: Duration::new(0, 0),
        }
    }

    pub fn update(&mut self, delay: Duration) {
        let now = Instant::now();
        if let Some(new_elapsed) = self.expected_elapsed.checked_add(delay) {
            self.expected_elapsed = new_elapsed;
            if let Some(notification_delay) = now.checked_duration_since(self.notification_time) {
                if notification_delay.as_secs() >= self.notification_delay {
                    self.notification_time = now;
                    if let Some(actual_elapsed) = now.checked_duration_since(self.start_time) {
                        let drift = actual_elapsed.as_secs_f64() - new_elapsed.as_secs_f64();
                        log::debug!("drift {} seconds", drift);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use super::Timer;

    pub fn new_in_the_past(delay: Duration, past_offset: Duration) -> Timer {
        Timer {
            delay,
            stop_watch: Instant::now().checked_sub(past_offset).unwrap(),
            remaining: Some(delay),
        }
    }

    #[test]
    fn create_timer() {
        let delay = Duration::new(1, 0);
        let mut timer = Timer::new(delay, false);
        let two_ms = Duration::new(0, 2 * 1_000_000);
        sleep(two_ms);
        let remaining = timer.remaining().unwrap();
        assert!(remaining < delay);
    }

    #[cfg(feature = "tui")]
    #[test]
    fn set_delay() {
        const SHORT_DELAY_VALUE: u64 = 60;
        const LONG_DELAY_VALUE: u64 = SHORT_DELAY_VALUE * 2;
        let short_delay = Duration::new(SHORT_DELAY_VALUE, 0);
        let long_delay = Duration::new(LONG_DELAY_VALUE, 0);
        // Set delay on expired timer.
        let mut timer1 = Timer::new(short_delay, true);
        assert_eq!(timer1.get_delay(), short_delay);
        timer1.set_delay(long_delay);
        assert_eq!(timer1.get_delay(), long_delay);

        // Set smaller delay
        // From 120 seconds (remaining > 60) to 60 seconds (remaining <= 60)
        let mut timer2 = Timer::new(long_delay, false);
        assert!(timer2.remaining().unwrap().as_secs() > SHORT_DELAY_VALUE);
        timer2.set_delay(short_delay);
        assert!(timer2.remaining().unwrap().as_secs() <= SHORT_DELAY_VALUE);

        // Set bigger delay
        // From 60 seconds (remaining <= 60) to 120 seconds (remaining > 60 and <= 120)
        let mut timer3 = Timer::new(short_delay, false);
        assert!(timer3.remaining().unwrap().as_secs() <= SHORT_DELAY_VALUE);
        timer3.set_delay(long_delay);
        let secs3 = timer3.remaining().unwrap().as_secs();
        assert!(secs3 > SHORT_DELAY_VALUE);
        assert!(secs3 <= LONG_DELAY_VALUE);
    }

    #[test]
    fn expired_timer() {
        let mut timer1 = Timer::new(Duration::new(60, 0), false);
        assert!(!timer1.expired());
        let mut timer2 = Timer::new(Duration::new(60, 0), true);
        assert!(timer2.expired());
    }

    #[test]
    fn remaining_time() {
        const DELAY_VALUE: u64 = 60;
        let delay = Duration::new(DELAY_VALUE, 0);
        // Non expired timer
        let mut timer1 = Timer::new(delay, false);
        for _ in 0..2 {
            assert!(timer1.remaining().unwrap().as_secs() > DELAY_VALUE / 2); // not expired
        }

        // expired timer
        let mut timer2 = new_in_the_past(delay, delay);
        assert!(timer2.remaining().is_none()); // expired
    }
}
