//! Days of the proleptic Gregorian calendar and times of day, for the dates of JavaScript
//! and Qt: a day from a year, a month and a day of the month, its day of the week, days
//! before and after it, seconds since the epoch and back.
//!
//! The years are -262143 to 262142, the range the code that reads these was written for
//! (`CIVIL_MS` in builtins.rs brings a time value outside it 400-year cycles nearer).

/// The first and the last year a [`Date`] holds.
pub const MIN_YEAR: i32 = -262_143;
pub const MAX_YEAR: i32 = 262_142;

const SECS_PER_DAY: i64 = 86_400;

/// The days from 1970-01-01 to the day `d` of month `m` of year `y` (Howard Hinnant's
/// days_from_civil: an era is 400 years, 146097 days, from a 1 March).
const fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The year, the month and the day of the month `days` days after 1970-01-01.
const fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

const fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

const fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

const MIN_DAYS: i64 = days_from_civil(MIN_YEAR as i64, 1, 1);
const MAX_DAYS: i64 = days_from_civil(MAX_YEAR as i64, 12, 31);

/// A day of the week.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weekday(u32);

impl Weekday {
    /// 1 for Monday to 7 for Sunday.
    pub fn number_from_monday(self) -> u32 {
        self.0 + 1
    }

    /// 0 for Monday to 6 for Sunday.
    pub fn num_days_from_monday(self) -> u32 {
        self.0
    }

    /// 0 for Sunday to 6 for Saturday.
    pub fn num_days_from_sunday(self) -> u32 {
        (self.0 + 1) % 7
    }
}

/// A day of the calendar: the days since 1970-01-01.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(i64);

impl Date {
    /// The day `day` of month `month` (1 to 12) of `year`; none for a day the month does not
    /// have and for a year outside [`MIN_YEAR`] to [`MAX_YEAR`].
    pub fn from_ymd_opt(year: i32, month: u32, day: u32) -> Option<Date> {
        let held = (MIN_YEAR..=MAX_YEAR).contains(&year) && (1..=12).contains(&month);
        (held && (1..=days_in_month(year, month)).contains(&day))
            .then(|| Date(days_from_civil(year.into(), month, day)))
    }

    fn from_days(days: i64) -> Option<Date> {
        (MIN_DAYS..=MAX_DAYS).contains(&days).then_some(Date(days))
    }

    pub fn year(self) -> i32 {
        civil_from_days(self.0).0 as i32
    }

    /// 1 to 12.
    pub fn month(self) -> u32 {
        civil_from_days(self.0).1
    }

    /// The day of the month, from 1.
    pub fn day(self) -> u32 {
        civil_from_days(self.0).2
    }

    pub fn leap_year(self) -> bool {
        is_leap(self.year())
    }

    pub fn weekday(self) -> Weekday {
        // 1970-01-01 was a Thursday.
        Weekday((self.0 + 3).rem_euclid(7) as u32)
    }

    /// The day `days` days later; none past the last year.
    pub fn checked_add_days(self, days: u64) -> Option<Date> {
        Date::from_days(self.0.checked_add(i64::try_from(days).ok()?)?)
    }

    /// The day `days` days earlier; none before the first year.
    pub fn checked_sub_days(self, days: u64) -> Option<Date> {
        Date::from_days(self.0.checked_sub(i64::try_from(days).ok()?)?)
    }

    /// The next day; none after the last one.
    pub fn succ_opt(self) -> Option<Date> {
        self.checked_add_days(1)
    }

    pub fn and_time(self, time: Time) -> DateTime {
        DateTime { date: self, time }
    }
}

/// A time of day: the seconds since midnight and the nanoseconds of the second. As a clock
/// that shows a leap second, the last second of a minute may hold up to 1_999_999_999 ns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time {
    secs: u32,
    nanos: u32,
}

impl Time {
    /// Midnight.
    pub const MIN: Time = Time { secs: 0, nanos: 0 };

    pub fn from_hms_opt(hour: u32, minute: u32, second: u32) -> Option<Time> {
        Time::from_hms_milli_opt(hour, minute, second, 0)
    }

    /// None for an hour past 23, a minute or a second past 59, and for 1000 ms and more in
    /// a second that is not the 59th (there up to 1999, the leap second).
    pub fn from_hms_milli_opt(hour: u32, minute: u32, second: u32, milli: u32) -> Option<Time> {
        let held = hour < 24 && minute < 60 && second < 60;
        let nanos = milli.checked_mul(1_000_000)?;
        (held && (nanos < 1_000_000_000 || (second == 59 && nanos < 2_000_000_000))).then_some(
            Time {
                secs: hour * 3600 + minute * 60 + second,
                nanos,
            },
        )
    }

    /// None for a day's 86400 seconds and more, and for a second of 1e9 ns and more that is
    /// not the 59th of its minute (there up to 2e9, the leap second).
    pub fn from_num_seconds_from_midnight_opt(secs: u32, nanos: u32) -> Option<Time> {
        let held =
            secs < 86_400 && (nanos < 1_000_000_000 || (secs % 60 == 59 && nanos < 2_000_000_000));
        held.then_some(Time { secs, nanos })
    }

    pub fn hour(self) -> u32 {
        self.secs / 3600
    }

    pub fn minute(self) -> u32 {
        self.secs / 60 % 60
    }

    pub fn second(self) -> u32 {
        self.secs % 60
    }

    pub fn nanosecond(self) -> u32 {
        self.nanos
    }
}

/// A day and a time of day, of no zone: read as UTC by [`DateTime::timestamp`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    pub date: Date,
    pub time: Time,
}

impl DateTime {
    /// The day and the time `secs` seconds and `nanos` ns after 1970-01-01 00:00 UTC; none
    /// outside the years held and for 2e9 ns and more (1e9 and more is a leap second: only
    /// in the 59th second of a minute).
    pub fn from_timestamp(secs: i64, nanos: u32) -> Option<DateTime> {
        let date = Date::from_days(secs.div_euclid(SECS_PER_DAY))?;
        let time =
            Time::from_num_seconds_from_midnight_opt(secs.rem_euclid(SECS_PER_DAY) as u32, nanos)?;
        Some(DateTime { date, time })
    }

    pub fn from_timestamp_millis(millis: i64) -> Option<DateTime> {
        DateTime::from_timestamp(
            millis.div_euclid(1000),
            millis.rem_euclid(1000) as u32 * 1_000_000,
        )
    }

    /// The seconds since 1970-01-01 00:00 UTC.
    pub fn timestamp(self) -> i64 {
        self.date.0 * SECS_PER_DAY + i64::from(self.time.secs)
    }

    /// The ms since 1970-01-01 00:00 UTC.
    pub fn timestamp_millis(self) -> i64 {
        self.timestamp() * 1000 + i64::from(self.time.nanos / 1_000_000)
    }

    pub fn year(self) -> i32 {
        self.date.year()
    }
}

/// The seconds since 1970-01-01 00:00 UTC now, by the system's clock (`CLOCK_REALTIME`,
/// what `std::time::SystemTime` reads); negative before 1970.
pub fn now_utc_secs() -> i64 {
    let now = std::time::SystemTime::now();
    match now.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        // The whole second at or before the reading.
        Err(e) => {
            let d = e.duration();
            -(d.as_secs() as i64) - i64::from(d.subsec_nanos() > 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ymd(d: Date) -> (i32, u32, u32) {
        (d.year(), d.month(), d.day())
    }

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd_opt(y, m, d).unwrap_or_else(|| panic!("{y}-{m}-{d}"))
    }

    /// Seconds since the epoch of midnight of a day.
    fn secs(y: i32, m: u32, d: u32) -> i64 {
        date(y, m, d).and_time(Time::MIN).timestamp()
    }

    #[test]
    fn known_days_are_their_seconds_and_weekdays() {
        // (year, month, day, seconds at midnight UTC, weekday from Monday = 1)
        let known = [
            (1970, 1, 1, 0, 4),
            (1969, 12, 31, -86_400, 3),
            (2000, 2, 29, 951_782_400, 2),
            (2000, 3, 1, 951_868_800, 3),
            (2024, 2, 29, 1_709_164_800, 4),
            (2026, 11, 17, 1_794_873_600, 2),
            (2038, 1, 19, 2_147_472_000, 2),
            (1900, 3, 1, -2_203_891_200, 4),
            (1600, 2, 29, -11_670_998_400, 2),
            (1, 1, 1, -62_135_596_800, 1),
            (0, 1, 1, -62_167_219_200, 6),
            (0, 2, 29, -62_162_121_600, 2),
            (-1, 12, 31, -62_167_305_600, 5),
            (9999, 12, 31, 253_402_214_400, 5),
            (-262_143, 1, 1, -8_334_601_228_800, 4),
            (262_142, 12, 31, 8_210_266_790_400, 1),
        ];
        for (y, m, d, at, weekday) in known {
            let day = date(y, m, d);
            assert_eq!(day.and_time(Time::MIN).timestamp(), at, "{y}-{m}-{d}");
            assert_eq!(day.weekday().number_from_monday(), weekday, "{y}-{m}-{d}");
            assert_eq!(day.weekday().num_days_from_monday(), weekday - 1);
            assert_eq!(day.weekday().num_days_from_sunday(), weekday % 7);
            let back = DateTime::from_timestamp(at, 0).unwrap();
            assert_eq!((ymd(back.date), back.time), ((y, m, d), Time::MIN));
            let last = DateTime::from_timestamp(at + 86_399, 0).unwrap();
            assert_eq!(ymd(last.date), (y, m, d));
            assert_eq!(
                (last.time.hour(), last.time.minute(), last.time.second()),
                (23, 59, 59)
            );
        }
    }

    #[test]
    fn a_day_no_month_has_is_none() {
        for (y, m, d) in [
            (2023, 2, 29),
            (2024, 2, 30),
            (1900, 2, 29),
            (2100, 2, 29),
            (2024, 4, 31),
            (2024, 0, 1),
            (2024, 13, 1),
            (2024, 1, 0),
            (2024, 1, 32),
            (-262_144, 12, 31),
            (262_143, 1, 1),
            (i32::MIN, 1, 1),
            (i32::MAX, 1, 1),
        ] {
            assert_eq!(Date::from_ymd_opt(y, m, d), None, "{y}-{m}-{d}");
        }
        for (y, leap) in [
            (2000, true),
            (1900, false),
            (2024, true),
            (2023, false),
            (0, true),
        ] {
            assert_eq!(date(y, 1, 1).leap_year(), leap, "{y}");
            assert_eq!(Date::from_ymd_opt(y, 2, 29).is_some(), leap, "{y}");
        }
        assert!(!date(-100, 1, 1).leap_year());
        assert!(date(-400, 1, 1).leap_year());
    }

    #[test]
    fn every_day_of_eight_centuries_follows_the_one_before() {
        let (mut day, mut weekday) = (date(1600, 1, 1), 6);
        let mut count = 0;
        for y in 1600..2400 {
            for m in 1..=12 {
                let days = match m {
                    2 if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) => 29,
                    2 => 28,
                    4 | 6 | 9 | 11 => 30,
                    _ => 31,
                };
                for d in 1..=days {
                    assert_eq!(ymd(day), (y, m, d));
                    assert_eq!(day, date(y, m, d));
                    assert_eq!(day.weekday().number_from_monday(), weekday, "{y}-{m}-{d}");
                    weekday = weekday % 7 + 1;
                    day = day.succ_opt().unwrap();
                    count += 1;
                }
                assert_eq!(Date::from_ymd_opt(y, m, days + 1), None);
            }
        }
        // Two 400-year cycles of 146097 days.
        assert_eq!(count, 2 * 146_097);
        assert_eq!(secs(2400, 1, 1) - secs(1600, 1, 1), 2 * 146_097 * 86_400);
    }

    #[test]
    fn days_are_added_and_taken_within_the_years_held() {
        let d = date(2024, 2, 28);
        assert_eq!(ymd(d.checked_add_days(1).unwrap()), (2024, 2, 29));
        assert_eq!(ymd(d.checked_add_days(2).unwrap()), (2024, 3, 1));
        assert_eq!(ymd(d.checked_add_days(366).unwrap()), (2025, 2, 28));
        assert_eq!(ymd(d.checked_sub_days(59).unwrap()), (2023, 12, 31));
        assert_eq!(ymd(d.checked_sub_days(0).unwrap()), (2024, 2, 28));
        assert_eq!(ymd(d.checked_add_days(146_097).unwrap()), (2424, 2, 28));
        let (first, last) = (date(MIN_YEAR, 1, 1), date(MAX_YEAR, 12, 31));
        assert_eq!(first.checked_sub_days(1), None);
        assert_eq!(last.checked_add_days(1), None);
        assert_eq!(last.succ_opt(), None);
        assert_eq!(first.checked_add_days(191_491_528), Some(last));
        assert_eq!(last.checked_sub_days(191_491_528), Some(first));
        assert_eq!(d.checked_add_days(u64::MAX), None);
        assert_eq!(d.checked_sub_days(u64::MAX), None);
    }

    #[test]
    fn seconds_and_ms_outside_the_years_held_are_none() {
        let (first, last) = (secs(MIN_YEAR, 1, 1), secs(MAX_YEAR, 12, 31) + 86_399);
        assert_eq!((first, last), (-8_334_601_228_800, 8_210_266_876_799));
        assert_eq!(
            ymd(DateTime::from_timestamp(first, 0).unwrap().date),
            (MIN_YEAR, 1, 1)
        );
        assert_eq!(
            ymd(DateTime::from_timestamp(last, 0).unwrap().date),
            (MAX_YEAR, 12, 31)
        );
        for out in [first - 1, last + 1, i64::MIN, i64::MAX] {
            assert_eq!(DateTime::from_timestamp(out, 0), None, "{out}");
        }
        assert!(DateTime::from_timestamp_millis(first * 1000).is_some());
        assert!(DateTime::from_timestamp_millis(last * 1000 + 999).is_some());
        for out in [
            first * 1000 - 1,
            last * 1000 + 1000,
            i64::MIN,
            i64::MAX,
            8_640_000_000_000_000,
        ] {
            assert_eq!(DateTime::from_timestamp_millis(out), None, "{out}");
        }
        // 8.2e15 ms, the limit builtins.rs keeps to, is held on both sides.
        for held in [8_200_000_000_000_000, -8_200_000_000_000_000] {
            let at = DateTime::from_timestamp_millis(held).unwrap();
            assert_eq!(at.timestamp_millis(), held);
        }
        assert_eq!(
            DateTime::from_timestamp_millis(8_200_000_000_000_000)
                .unwrap()
                .year(),
            261_817
        );
        assert_eq!(
            DateTime::from_timestamp_millis(-8_200_000_000_000_000)
                .unwrap()
                .year(),
            -257_878
        );
    }

    #[test]
    fn ms_before_the_epoch_are_the_end_of_the_day_before() {
        let at = DateTime::from_timestamp_millis(-1).unwrap();
        assert_eq!(ymd(at.date), (1969, 12, 31));
        let t = at.time;
        assert_eq!(
            (t.hour(), t.minute(), t.second(), t.nanosecond()),
            (23, 59, 59, 999_000_000)
        );
        assert_eq!((at.timestamp(), at.timestamp_millis()), (-1, -1));
        let at = DateTime::from_timestamp_millis(1_794_924_123_456).unwrap();
        assert_eq!(ymd(at.date), (2026, 11, 17));
        let t = at.time;
        assert_eq!(
            (t.hour(), t.minute(), t.second(), t.nanosecond()),
            (14, 2, 3, 456_000_000)
        );
        assert_eq!(at.timestamp_millis(), 1_794_924_123_456);
        assert_eq!(
            DateTime::from_timestamp(-86_401, 0).unwrap().timestamp(),
            -86_401
        );
        assert_eq!(
            ymd(DateTime::from_timestamp(-86_401, 0).unwrap().date),
            (1969, 12, 30)
        );
    }

    #[test]
    fn times_of_day() {
        let hms = |t: Time| (t.hour(), t.minute(), t.second(), t.nanosecond());
        assert_eq!(hms(Time::MIN), (0, 0, 0, 0));
        assert_eq!(Time::from_hms_opt(0, 0, 0), Some(Time::MIN));
        assert_eq!(
            hms(Time::from_hms_opt(23, 59, 59).unwrap()),
            (23, 59, 59, 0)
        );
        assert_eq!(
            hms(Time::from_hms_milli_opt(10, 20, 30, 500).unwrap()),
            (10, 20, 30, 500_000_000)
        );
        for (h, m, s) in [(24, 0, 0), (0, 60, 0), (0, 0, 60), (u32::MAX, 0, 0)] {
            assert_eq!(Time::from_hms_opt(h, m, s), None, "{h}:{m}:{s}");
        }
        // 1000 ms and more are a leap second: in the 59th second only, up to 1999.
        assert_eq!(Time::from_hms_milli_opt(10, 20, 30, 1000), None);
        assert_eq!(
            hms(Time::from_hms_milli_opt(10, 20, 59, 1999).unwrap()),
            (10, 20, 59, 1_999_000_000)
        );
        assert_eq!(Time::from_hms_milli_opt(10, 20, 59, 2000), None);
        assert_eq!(Time::from_hms_milli_opt(10, 20, 59, u32::MAX), None);
        assert_eq!(
            hms(Time::from_num_seconds_from_midnight_opt(86_399, 999_000_000).unwrap()),
            (23, 59, 59, 999_000_000)
        );
        assert_eq!(
            hms(Time::from_num_seconds_from_midnight_opt(3723, 4_000_000).unwrap()),
            (1, 2, 3, 4_000_000)
        );
        assert_eq!(Time::from_num_seconds_from_midnight_opt(86_400, 0), None);
        assert_eq!(
            Time::from_num_seconds_from_midnight_opt(3723, 1_000_000_000),
            None
        );
        assert!(Time::from_num_seconds_from_midnight_opt(3719, 1_999_999_999).is_some());
        assert_eq!(
            Time::from_num_seconds_from_midnight_opt(3719, 2_000_000_000),
            None
        );
        assert!(Time::from_hms_opt(1, 2, 3) < Time::from_hms_milli_opt(1, 2, 3, 1));
    }

    #[test]
    fn now_is_the_system_clock() {
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let now = now_utc_secs();
        assert!((before..=before + 1).contains(&now), "{before} {now}");
    }
}
