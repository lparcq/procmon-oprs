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

// Value unchanged
pub fn identity(value: u64) -> String {
    format!("{}", value)
}

// Value in Kibi
pub fn kibi(value: u64) -> String {
    format!("{:.1} Ki", (value as f64) / KIBI)
}

// Value in Mebi
pub fn mebi(value: u64) -> String {
    format!("{:.1} Mi", (value as f64) / MEBI)
}

// Value in Gibi
pub fn gibi(value: u64) -> String {
    format!("{:.1} Gi", (value as f64) / GIBI)
}

// Value in Tebi
pub fn tebi(value: u64) -> String {
    format!("{:.1} Ti", (value as f64) / TEBI)
}

// Float value in Kilo
fn kilo_f(value: f64) -> String {
    format!("{:.1} K", value / KILO_F)
}

// Value in Kilo
pub fn kilo(value: u64) -> String {
    kilo_f(value as f64)
}

// Float value in Mega
pub fn mega_f(value: f64) -> String {
    format!("{:.1} M", value / MEGA_F)
}

// Value in Mega
pub fn mega(value: u64) -> String {
    mega_f(value as f64)
}

// Float value in Giga
pub fn giga_f(value: f64) -> String {
    format!("{:.1} G", value / GIGA_F)
}

// Value in Giga
pub fn giga(value: u64) -> String {
    giga_f(value as f64)
}

// Float value in Tera
pub fn tera_f(value: f64) -> String {
    format!("{:.1} T", value / TERA_F)
}

// Value in Tera
pub fn tera(value: u64) -> String {
    tera_f(value as f64)
}

// Integer value formatted using the best unit in Kilo, Mega, Giga
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

// Number of seconds and fraction of milliseconds
pub fn duration_seconds(millis: u64) -> String {
    let seconds = millis / 1000;
    let remaining_millis = millis - seconds * 1000;
    format!("{}.{}", seconds, remaining_millis)
}

// Number of milliseconds formatted in hms.
pub fn duration_human(millis: u64) -> String {
    if millis < 1000 {
        format!("{}ms", millis)
    } else {
        let seconds = millis / 1000;
        let remaining_millis = millis - seconds * 1000;
        if seconds < 60 {
            if remaining_millis > 0 {
                format!("{}s {}ms", seconds, remaining_millis)
            } else {
                format!("{}s", seconds)
            }
        } else {
            let minutes = seconds / 60;
            let remaining_seconds = seconds - minutes * 60;
            if minutes < 60 {
                format!("{}m {}s", minutes, remaining_seconds)
            } else {
                let hours = minutes / 60;
                let remaining_minutes = minutes - hours * 60;
                if hours < 24 {
                    format!("{}h {}m {}s", hours, remaining_minutes, remaining_seconds)
                } else {
                    format!("{}h {}m", hours, remaining_minutes)
                }
            }
        }
    }
}

// Percentage multiplied by 1000 (i.e. 1000 = 100%)
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
    fn test_duration_seconds() {
        assert_eq!("59.150", super::duration_seconds(59150));
    }

    #[test]
    fn test_duration_human() {
        let seconds_millis = 1000;
        let minutes_millis = 60 * seconds_millis;
        let hour_millis = 60 * minutes_millis;
        assert_eq!("59s", super::duration_human(59 * seconds_millis));
        assert_eq!("59s 150ms", super::duration_human(59150));
        assert_eq!("1m 15s", super::duration_human(75 * seconds_millis));
        assert_eq!(
            "59m 59s",
            super::duration_human(59 * minutes_millis + 59 * seconds_millis)
        );
        assert_eq!(
            "3h 5m 10s",
            super::duration_human((((3 * 60) + 5) * 60 + 10) * 1000)
        );
        assert_eq!(
            "3h 5m 10s",
            super::duration_human(3 * hour_millis + 5 * minutes_millis + 10 * seconds_millis)
        );
        assert_eq!(
            "26h 5m",
            super::duration_human(26 * hour_millis + 5 * minutes_millis + 10 * seconds_millis)
        );
    }
}
