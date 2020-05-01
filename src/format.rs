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

// Number of seconds formatted in hms.
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
        assert_eq!("1.0 T", super::size(1_000_000_000_000));
    }

    #[test]
    fn test_duration() {
        assert_eq!("59s", super::duration(59));
        assert_eq!("1m 15s", super::duration(75));
        assert_eq!("59m 59s", super::duration(3599));
        assert_eq!("3h 5m 10s", super::duration(((3 * 60) + 5) * 60 + 10));
    }
}
