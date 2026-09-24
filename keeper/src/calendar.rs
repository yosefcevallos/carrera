//! NYSE regular session: 09:30–16:00 America/New_York, Monday to Friday.
//! Exchange holidays are not modelled; the program's own market flag is authoritative
//! once Phoenix's calendar is wired (spec §7.5).

use chrono::{DateTime, Datelike, NaiveTime, Timelike, Utc, Weekday};
use chrono_tz::America::New_York;

pub fn market_open(now: DateTime<Utc>) -> bool {
    let ny = now.with_timezone(&New_York);
    if matches!(ny.weekday(), Weekday::Sat | Weekday::Sun) {
        return false;
    }
    let t = NaiveTime::from_hms_opt(ny.hour(), ny.minute(), ny.second()).expect("valid time");
    let open = NaiveTime::from_hms_opt(9, 30, 0).unwrap();
    let close = NaiveTime::from_hms_opt(16, 0, 0).unwrap();
    t >= open && t < close
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ny(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        New_York.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn session_boundaries() {
        // Wed 15 Jan 2026 (EST) and Wed 15 Jul 2026 (EDT).
        for (m, d) in [(1, 15), (7, 15)] {
            assert!(!market_open(ny(2026, m, d, 9, 29)));
            assert!(market_open(ny(2026, m, d, 9, 30)));
            assert!(market_open(ny(2026, m, d, 15, 59)));
            assert!(!market_open(ny(2026, m, d, 16, 0)));
        }
    }

    #[test]
    fn weekends_closed() {
        assert!(!market_open(ny(2026, 9, 26, 12, 0))); // Saturday
        assert!(!market_open(ny(2026, 9, 27, 12, 0))); // Sunday
        assert!(market_open(ny(2026, 9, 28, 12, 0))); // Monday
    }

    #[test]
    fn utc_conversion_is_applied() {
        // 14:00 UTC in July is 10:00 EDT (open); in January it is 09:00 EST (closed).
        assert!(market_open(Utc.with_ymd_and_hms(2026, 7, 15, 14, 0, 0).unwrap()));
        assert!(!market_open(Utc.with_ymd_and_hms(2026, 1, 15, 14, 0, 0).unwrap()));
    }
}
