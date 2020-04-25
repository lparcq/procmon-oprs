pub type Formatter = fn(u64) -> String;

const KIBI: f64 = 1024.0;
const MEBI: f64 = KIBI * KIBI;
const GIBI: f64 = MEBI * KIBI;

const KILO_U: u64 = 1000;
const KILO_F: f64 = 1000.0;
const MEGA_F: f64 = KILO_F * KILO_F;
const GIGA_F: f64 = MEGA_F * KILO_F;

pub fn identity(value: u64) -> String {
    format!("{}", value)
}

pub fn kibi(value: u64) -> String {
    format!("{:.1} Ki", (value as f64) / KIBI)
}

pub fn mebi(value: u64) -> String {
    format!("{:.1} Mi", (value as f64) / MEBI)
}

pub fn gibi(value: u64) -> String {
    format!("{:.1} Gi", (value as f64) / GIBI)
}

fn kilo_f(value: f64) -> String {
    format!("{:.1} K", value / KILO_F)
}

pub fn kilo(value: u64) -> String {
    kilo_f(value as f64)
}

pub fn mega_f(value: f64) -> String {
    format!("{:.1} M", value / MEGA_F)
}

pub fn mega(value: u64) -> String {
    mega_f(value as f64)
}

pub fn giga_f(value: f64) -> String {
    format!("{:.1} G", value / GIGA_F)
}

pub fn giga(value: u64) -> String {
    giga_f(value as f64)
}

pub fn size(value: u64) -> String {
    if value < KILO_U {
        identity(value)
    } else {
        let fvalue = value as f64;
        if fvalue < MEGA_F {
            kilo_f(fvalue)
        } else if fvalue < GIGA_F {
            mega_f(fvalue)
        } else {
            giga_f(fvalue)
        }
    }
}

pub fn duration(value: u64) -> String {
    if value < 60 {
        format!("{}s", value)
    } else {
        let minutes = value / 60;
        let seconds = value - minutes * 60;
        if minutes < 60 {
            format!("{}m {}s", minutes, seconds)
        } else {
            let hours = minutes / 60;
            let minutes = minutes - hours * 60;
            format!("{}h {}m {}s", hours, minutes, seconds)
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_size() {
        assert_eq!("512", super::size(512));
        assert_eq!("1.0 K", super::size(1_000));
        assert_eq!("1.0 M", super::size(1_000_000));
        assert_eq!("1.0 G", super::size(1_000_000_000));
    }

    #[test]
    fn test_duration() {
        assert_eq!("59s", super::duration(59));
        assert_eq!("1m 15s", super::duration(75));
        assert_eq!("59m 59s", super::duration(3599));
        assert_eq!("3h 5m 10s", super::duration(((3 * 60) + 5) * 60 + 10));
    }
}
