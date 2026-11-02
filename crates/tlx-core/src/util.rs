//! Utilità interne: tempo, troncamento, formattazione data in stile C.

use std::time::{SystemTime, UNIX_EPOCH};

/// Millisecondi dall'epoch Unix.
pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Tronca una stringa a `max` caratteri aggiungendo `…`.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

/// Data in stile `ctime()` (UTC), es. `Thu Jul  1 11:23:52 2010`.
pub fn ctime_utc(secs: u64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // giorni → data civile (algoritmo di H. Hinnant)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!(
        "{} {} {:>2} {:02}:{:02}:{:02} {}",
        DAYS[(days % 7) as usize],
        MONTHS[(month - 1) as usize],
        day,
        h,
        m,
        s,
        year
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctime() {
        // 1 luglio 2010 11:23:52 UTC
        assert_eq!(ctime_utc(1_277_983_432), "Thu Jul  1 11:23:52 2010");
        assert_eq!(ctime_utc(0), "Thu Jan  1 00:00:00 1970");
    }

    #[test]
    fn tronca() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 3), "abc…");
    }
}
