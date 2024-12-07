// Oprs -- process monitor for Linux
// Copyright (C) 2020  Laurent Pelecq
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

use std::time::Duration;

pub type Formatter = fn(u64) -> String;

const KIBI: f64 = 1024.0;
const MEBI: f64 = KIBI * KIBI;
const GIBI: f64 = MEBI * KIBI;
const TEBI: f64 = GIBI * KIBI;

const KILO_U: u64 = 1000;
const KILO_F: f64 = 1000.0;
const MEGA_F: f64 = KILO_F * KILO_F;
const GIGA_F: f64 = MEGA_F * KILO_F;
const TERA_F: f64 = GIGA_F * KILO_F;

/// Value unchanged
pub fn identity(value: u64) -> String {
    format!("{value}")
}

/// Value in Kibi
pub fn kibi(value: u64) -> String {
    format!("{:.2} Ki", (value as f64) / KIBI)
}

/// Value in Mebi
pub fn mebi(value: u64) -> String {
    format!("{:.2} Mi", (value as f64) / MEBI)
}

/// Value in Gibi
pub fn gibi(value: u64) -> String {
    format!("{:.2} Gi", (value as f64) / GIBI)
}

/// Value in Tebi
pub fn tebi(value: u64) -> String {
    format!("{:.2} Ti", (value as f64) / TEBI)
}

/// Float value in Kilo
fn kilo_f(value: f64) -> String {
    format!("{:.2} K", value / KILO_F)
}

/// Value in Kilo
pub fn kilo(value: u64) -> String {
    kilo_f(value as f64)
}

/// Float value in Mega
pub fn mega_f(value: f64) -> String {
    format!("{:.2} M", value / MEGA_F)
}

/// Value in Mega
pub fn mega(value: u64) -> String {
    mega_f(value as f64)
}

/// Float value in Giga
pub fn giga_f(value: f64) -> String {
    format!("{:.2} G", value / GIGA_F)
}

/// Value in Giga
pub fn giga(value: u64) -> String {
    giga_f(value as f64)
}

/// Float value in Tera
pub fn tera_f(value: f64) -> String {
    format!("{:.2} T", value / TERA_F)
}

/// Value in Tera
pub fn tera(value: u64) -> String {
    tera_f(value as f64)
}

/// Integer value formatted using the best unit in Kilo, Mega, Giga
pub fn size(value: u64) -> String {
    if value < KILO_U {
        identity(value)
    } else {
        let fvalue = value as f64;
        if fvalue < MEGA_F {
            kilo_f(fvalue)
        } else if fvalue < GIGA_F {
            mega_f(fvalue)
        } else if fvalue < TERA_F {
            giga_f(fvalue)
        } else {
            tera_f(fvalue)
        }
    }
}

/// Number of seconds and fraction of milliseconds
pub fn seconds(millis: u64) -> String {
    let seconds = millis / 1000;
    let remaining_millis = millis - seconds * 1000;
    format!("{seconds}.{remaining_millis}")
}

/// Number of milliseconds formatted in human readable format (hms).
pub fn human_milliseconds(millis: u64) -> String {
    if millis < 1000 {
        format!("{millis}ms")
    } else {
        let seconds = millis / 1000;
        let remaining_millis = millis - seconds * 1000;
        if seconds < 60 {
            if remaining_millis > 0 {
                format!("{seconds}s {remaining_millis}ms")
            } else {
                format!("{seconds}s")
            }
        } else {
            let minutes = seconds / 60;
            let remaining_seconds = seconds - minutes * 60;
            if minutes < 60 {
                format!("{minutes}m {remaining_seconds}s")
            } else {
                let hours = minutes / 60;
                let remaining_minutes = minutes - hours * 60;
                if hours < 24 {
                    format!("{hours}h {remaining_minutes:0>2}m {remaining_seconds:0>2}s")
                } else {
                    format!("{hours}h {remaining_minutes:0>2}m")
                }
            }
        }
    }
}

/// Duration in human readable format
pub fn human_duration(duration: Duration) -> String {
    let ms = duration.as_secs() * 1000 + duration.subsec_millis() as u64;
    human_milliseconds(ms)
}

/// Percentage multiplied by 1000 (i.e. 1000 = 100%)
pub fn ratio(value: u64) -> String {
    format!("{:.1}%", (value as f32) / 10.0)
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_size() {
        assert_eq!("512", super::size(512));
        assert_eq!("1.0 K", super::size(1_000));
        assert_eq!("1.0 M", super::size(1_000_000));
        assert_eq!("1.0 G", super::size(1_000_000_000));
        assert_eq!("1.0 T", super::size(1_000_000_000_000));
    }

    #[test]
    fn test_seconds() {
        assert_eq!("59.150", super::seconds(59150));
    }

    #[test]
    fn test_human_milliseconds() {
        let seconds_millis = 1000;
        let minutes_millis = 60 * seconds_millis;
        let hour_millis = 60 * minutes_millis;
        assert_eq!("59s", super::human_milliseconds(59 * seconds_millis));
        assert_eq!("59s 150ms", super::human_milliseconds(59150));
        assert_eq!("1m 15s", super::human_milliseconds(75 * seconds_millis));
        assert_eq!(
            "59m 59s",
            super::human_milliseconds(59 * minutes_millis + 59 * seconds_millis)
        );
        assert_eq!(
            "3h 5m 10s",
            super::human_milliseconds((((3 * 60) + 5) * 60 + 10) * 1000)
        );
        assert_eq!(
            "3h 5m 10s",
            super::human_milliseconds(3 * hour_millis + 5 * minutes_millis + 10 * seconds_millis)
        );
        assert_eq!(
            "26h 5m",
            super::human_milliseconds(26 * hour_millis + 5 * minutes_millis + 10 * seconds_millis)
        );
    }
}
