//! QDate, QTime and QDateTime `fromString(text, Qt::DateFormat)` as qtbase 6.12 reads them
//! (qdatetime.cpp): the texts `Qt.formatDate` and friends take for a date, and the ones V4's
//! `Date.parse` falls back to after the ECMAScript format.

use chrono::{NaiveDate, NaiveTime};

use crate::tz::{Database, Zone};

/// What a QDateTime's clock reading is relative to (Qt::TimeSpec).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spec {
    Local,
    Utc,
    /// Seconds ahead of UTC.
    Offset(i32),
}

impl Spec {
    /// QTimeZone::fromSecondsAheadOfUtc: no offset is UTC itself.
    fn ahead(secs: i32) -> Spec {
        match secs {
            0 => Spec::Utc,
            s => Spec::Offset(s),
        }
    }
}

/// A valid QDateTime: a calendar date and clock time, read in `spec`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QDateTime {
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub spec: Spec,
}

/// Qt::DateFormat's text forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateFormat {
    Text,
    Iso,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// QDate(year, month, day): a day of the proleptic Gregorian calendar, which has no year 0.
fn qdate(year: i32, month: i32, day: i32) -> Option<NaiveDate> {
    let year = match year {
        0 => return None,
        y if y < 0 => y + 1,
        y => y,
    };
    NaiveDate::from_ymd_opt(year, u32::try_from(month).ok()?, u32::try_from(day).ok()?)
}

/// readInt: a whole number that is the whole text, without sign or spaces.
fn read_int(text: &str) -> Option<u64> {
    let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    digits.then(|| text.parse().ok()).flatten()
}

/// QStringView::toInt of a word.
fn to_int(text: &str) -> Option<i32> {
    text.parse().ok()
}

fn short_month(name: &str) -> Option<i32> {
    MONTHS.iter().position(|m| *m == name).map(|i| i as i32 + 1)
}

/// The first `max` space-separated words; None when there are more.
fn words(s: &str, max: usize) -> Option<Vec<&str>> {
    let w: Vec<&str> = s.split(' ').filter(|w| !w.is_empty()).collect();
    (w.len() <= max).then_some(w)
}

/// fromOffsetString: `±HH[[:]mm]` as seconds ahead of UTC.
fn offset(text: &str) -> Option<i32> {
    if !text.is_ascii() || !(2..=6).contains(&text.len()) {
        return None;
    }
    let sign = match text.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let time = &text[1..];
    let (hh_len, mm_index) = match time.find(':') {
        Some(i) => (i, i + 1),
        None => (2, 2),
    };
    let hour = to_int(&time[..hh_len.min(time.len())]).filter(|h| *h <= 23)?;
    let minute = match &time[mm_index.min(time.len())..] {
        "" => 0,
        mm => to_int(mm).filter(|m| (0..=59).contains(m))?,
    };
    Some(sign * (hour * 60 + minute) * 60)
}

/// fromIsoTimeString: `HH[:mm[:ss]]` with an optional fraction of the last field given, and
/// whether it is ISO 8601's 24:00, the end of the day.
fn iso_time(text: &str, format: DateFormat) -> Option<(NaiveTime, bool)> {
    if !text.is_ascii() {
        return None;
    }
    let (dot, comma) = (text.find('.'), text.find(','));
    let (text, tail) = match (dot, comma) {
        (Some(i), _) if text[i + 1..].contains('.') => return None,
        (Some(i), _) | (None, Some(i)) => (&text[..i], &text[i + 1..]),
        (None, None) => (text, ""),
    };
    if tail.contains(',') {
        return None;
    }
    let frac = read_int(tail);
    if tail.is_empty() && (dot.is_some() || comma.is_some()) || !tail.is_empty() && frac.is_none() {
        return None;
    }
    let mut fraction = frac.map_or(0.0, |f| f as f64 * 0.1f64.powi(tail.len() as i32));
    if !(2..=8).contains(&text.len()) {
        return None;
    }
    let iso = format == DateFormat::Iso;
    let mut hour = read_int(&text[..2]).filter(|h| *h <= if iso { 24 } else { 23 })?;
    let mut minute = 0;
    if text.len() > 2 {
        if text.as_bytes()[2] != b':' || text.len() <= 4 {
            return None;
        }
        minute = read_int(&text[3..5]).filter(|m| *m < 60)?;
    } else if !iso {
        return None;
    } else if frac.is_some() {
        fraction *= 60.0;
        minute = fraction as u64;
        fraction -= minute as f64;
    }
    let mut second = 0;
    if text.len() > 5 {
        if text.as_bytes()[5] != b':' || text.len() != 8 {
            return None;
        }
        second = read_int(&text[6..8]).filter(|s| *s < 60)?;
    } else if frac.is_some() {
        if !iso {
            return None;
        }
        fraction *= 60.0;
        second = fraction as u64;
        fraction -= second as f64;
    }
    let mut msec = if frac.is_some() {
        (1000.0 * fraction).round() as u64
    } else {
        0
    };
    if msec == 1000 {
        if iso || hour < 23 || minute < 59 || second < 59 {
            msec = 0;
            second += 1;
            if second == 60 {
                second = 0;
                minute += 1;
                if minute == 60 {
                    minute = 0;
                    hour += 1;
                }
            }
        } else {
            msec = 999;
        }
    }
    let midnight24 = (hour, minute, second, msec) == (24, 0, 0, 0);
    if midnight24 {
        hour = 0;
    }
    let time =
        NaiveTime::from_hms_milli_opt(hour as u32, minute as u32, second as u32, msec as u32)?;
    Some((time, midnight24))
}

/// QTime::fromString(text, Qt::ISODate).
pub fn time_from_iso(text: &str) -> Option<NaiveTime> {
    iso_time(text, DateFormat::Iso).map(|(t, _)| t)
}

/// QDate::fromString(text, Qt::ISODate): `YYYY-MM-DD` with any punctuation between the fields
/// and no digit after them.
pub fn date_from_iso(text: &str) -> Option<NaiveDate> {
    let c: Vec<char> = text.chars().take(11).collect();
    let punct = |i: usize| {
        c[i].is_ascii_punctuation()
            || (!c[i].is_ascii() && !c[i].is_alphanumeric() && !c[i].is_whitespace())
    };
    if c.len() < 10 || !punct(4) || !punct(7) || c.get(10).is_some_and(|d| d.is_numeric()) {
        return None;
    }
    let field = |from: usize, to: usize| read_int(&c[from..to].iter().collect::<String>());
    let year = field(0, 4).filter(|y| (1..=9999).contains(y))?;
    qdate(year as i32, field(5, 7)? as i32, field(8, 10)? as i32)
}

/// The parts of an RFC 2822 (or 850) date-time, rfcDateImpl: `[ddd,] dd MMM yyyy [hh:mm[:ss]]
/// [±hhmm]` or `ddd MMM dd [hh:mm:ss] yyyy [±hhmm]`. A text of another shape is None; one of
/// the right shape has the date and time it names, where valid, and its offset.
fn rfc_parts(s: &str) -> Option<(Option<NaiveDate>, Option<NaiveTime>, i32)> {
    let mut words = words(s, 6).filter(|w| w.len() >= 3)?;
    let short_name = |name: &str| {
        let c: Vec<char> = name.chars().collect();
        c.len() == 3 && c[0].is_uppercase() && c[1].is_lowercase() && c[2].is_lowercase()
    };
    let (day_name, rfc_x22) = match words[0] {
        w if w.ends_with(',') => (Some(&w[..w.len() - 1]), true),
        w if !w.starts_with(|c: char| c.is_numeric()) => (Some(w), false),
        _ => (None, true),
    };
    if day_name.is_some() {
        words.remove(0);
    }
    if !(3..=5).contains(&words.len()) {
        return None;
    }
    let (day_index, month_index, year_index) = match rfc_x22 {
        true => (0, 1, 2),
        false if words.len() > 3 && words[2].contains(':') => (1, 0, 3),
        false => (1, 0, 2),
    };
    if words[year_index].chars().count() != 4 {
        return None;
    }
    let date = 'date: {
        let day_of_week = match day_name {
            Some(name) if !short_name(name) => return None,
            Some(name) => match DAYS.iter().position(|d| *d == name) {
                Some(i) => Some(i as u32),
                None => break 'date None,
            },
            None => None,
        };
        let day = to_int(words[day_index])?;
        let year = to_int(words[year_index])?;
        if !short_name(words[month_index]) {
            return None;
        }
        let Some(month) = short_month(words[month_index]) else {
            break 'date None;
        };
        use chrono::Datelike;
        qdate(year, month, day)
            .filter(|d| day_of_week.is_none_or(|w| d.weekday().num_days_from_monday() == w))
    };
    words.remove(year_index);
    words.drain(..2);
    let mut time = None;
    if words.first().is_some_and(|w| w.contains(':')) {
        let when = words.remove(0);
        let b = when.as_bytes();
        let shape = when.is_ascii()
            && b.len() >= 5
            && b[2] == b':'
            && if b.len() == 8 {
                b[5] == b':'
            } else {
                b.len() == 5
            };
        if !shape {
            return None;
        }
        let hour = to_int(&when[..2])?;
        let minute = to_int(&when[3..5])?;
        let second = match b.len() {
            8 => to_int(&when[6..])?,
            _ => 0,
        };
        time = (|| {
            NaiveTime::from_hms_opt(
                u32::try_from(hour).ok()?,
                u32::try_from(minute).ok()?,
                u32::try_from(second).ok()?,
            )
        })();
    }
    let mut ahead = 0;
    if !words.is_empty() {
        let zone = words.remove(0);
        if !words.is_empty() || !zone.is_ascii() || !matches!(zone.len(), 3 | 5) {
            return None;
        }
        let sign = match zone.as_bytes()[0] {
            b'-' => -1,
            b'+' => 1,
            _ => return None,
        };
        let hour = to_int(&zone[1..3])?;
        let minute = match zone.len() {
            5 => to_int(&zone[3..])?,
            _ => 0,
        };
        ahead = sign * (hour * 60 + minute) * 60;
    }
    Some((date, time, ahead))
}

impl QDateTime {
    /// QDateTime::fromString(text, Qt::RFC2822Date).
    pub fn from_rfc2822(text: &str) -> Option<QDateTime> {
        let (date, time, ahead) = rfc_parts(text)?;
        Some(QDateTime {
            date: date?,
            time: time?,
            spec: Spec::ahead(ahead),
        })
    }

    /// QDateTime::fromString(text, Qt::ISODate): a date alone is the start of that day.
    pub fn from_iso(text: &str) -> Option<QDateTime> {
        let split = text.char_indices().nth(10).map_or(text.len(), |(i, _)| i);
        if text.chars().count() < 10 {
            return None;
        }
        let date = date_from_iso(&text[..split])?;
        let rest = &text[split..];
        if rest.is_empty() {
            let time = NaiveTime::MIN;
            return Some(QDateTime {
                date,
                time,
                spec: Spec::Local,
            });
        }
        let mut rest = rest
            .strip_prefix(['T', 't', ' '])
            .filter(|r| !r.is_empty())?;
        let mut spec = Spec::Local;
        if let Some(r) = rest.strip_suffix(['Z', 'z']) {
            (rest, spec) = (r, Spec::Utc);
        } else if let Some(sign) = rest.rfind(['+', '-']) {
            spec = Spec::ahead(offset(&rest[sign..])?);
            rest = &rest[..sign];
        }
        let (time, midnight24) = iso_time(rest, DateFormat::Iso)?;
        let date = match midnight24 {
            true => date.succ_opt()?,
            false => date,
        };
        Some(QDateTime { date, time, spec })
    }

    /// QDateTime::fromString(text, Qt::TextDate): `ddd MMM d HH:mm:ss yyyy` or with the year
    /// before the time, then an optional `UTC` or `GMT` with an optional offset.
    pub fn from_text(text: &str) -> Option<QDateTime> {
        let parts = words(text, 6).filter(|p| p.len() >= 5)?;
        let (year_part, time_part) = match (parts[3].contains(':'), parts[4].contains(':')) {
            (true, _) => (4, 3),
            (false, true) => (3, 4),
            _ => return None,
        };
        let day = to_int(parts[2]).filter(|d| *d != 0)?;
        let year = to_int(parts[year_part]).filter(|y| *y != 0)?;
        let date = qdate(year, short_month(parts[1])?, day)?;
        let (time, _) = iso_time(parts[time_part], DateFormat::Text)?;
        let spec = match parts.get(5) {
            None => Spec::Local,
            Some(tz) => {
                let gmt = tz.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("GMT"));
                match &tz[if gmt || tz.starts_with("UTC") {
                    3
                } else {
                    return None;
                }..]
                {
                    "" => Spec::Utc,
                    ahead => Spec::ahead(offset(ahead)?),
                }
            }
        };
        Some(QDateTime { date, time, spec })
    }
}

const LONG_MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// One field of a `fromString(text, format)` format, with its letter count where that
/// changes how it is read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Year,
    Year2,
    Month(usize),
    Day(usize),
    Weekday,
    Hour(usize),
    Minute,
    Second,
    AmPm,
    Zone,
}

/// QDateTimeParser::parseFormat for the letters V4's formats use: the fields, and the text
/// before, between and after them.
fn fields(format: &str) -> (Vec<Field>, Vec<Vec<char>>) {
    let f: Vec<char> = format.chars().collect();
    let (mut fields, mut seps, mut i) = (Vec::new(), vec![Vec::new()], 0);
    while i < f.len() {
        let n = f[i..].iter().take_while(|c| **c == f[i]).count();
        let field = match (f[i], n) {
            ('y', 4) => Some(Field::Year),
            ('y', 2) => Some(Field::Year2),
            ('M', _) => Some(Field::Month(n)),
            ('d', 3) => Some(Field::Weekday),
            ('d', _) => Some(Field::Day(n)),
            ('h', _) => Some(Field::Hour(n)),
            ('m', _) => Some(Field::Minute),
            ('s', _) => Some(Field::Second),
            ('A', _) => Some(Field::AmPm),
            ('t', _) => Some(Field::Zone),
            _ => None,
        };
        match field {
            Some(field) => {
                fields.push(field);
                seps.push(Vec::new());
                i += n;
            }
            None => {
                seps.last_mut().unwrap().push(f[i]);
                i += 1;
            }
        }
    }
    (fields, seps)
}

/// matchesSeparator: how much of `text` the separator takes; any space stands for a space.
fn separator(text: &[char], sep: &[char]) -> Option<usize> {
    let space = |c: char| c == ' ' || (!c.is_ascii() && c.is_whitespace());
    let same = |(s, g): (&char, &char)| s == g || (space(*s) && space(*g));
    (text.len() >= sep.len() && sep.iter().zip(text).all(same)).then_some(sep.len())
}

/// A numeric field of parseSection: the longest run of at most `width` digits whose value is
/// at most `max`, and how many characters it took. A fixed-width field needs every digit; an
/// hour of a 12-hour clock is 1 to 12, where 12 is hour 0.
fn number(
    text: &[char],
    width: usize,
    max: i32,
    fixed: bool,
    twelve: bool,
) -> Option<(i32, usize)> {
    let digits = text
        .iter()
        .take(width)
        .take_while(|c| c.is_ascii_digit())
        .count();
    (1..=digits)
        .rev()
        .find_map(|n| {
            let value: i32 = text[..n].iter().collect::<String>().parse().ok()?;
            let value = match twelve {
                true if value == 0 || value > 12 => return None,
                true => value % 12,
                false => value,
            };
            (value <= max).then_some((value, n))
        })
        .filter(|(_, n)| !fixed || *n == width)
}

/// findTextEntry: the name the text starts with (ignoring case), counted from 1, and its
/// length. A name the text only begins to spell is no match.
fn name(text: &[char], names: &[&str]) -> Option<(i32, usize)> {
    let longest = names.iter().map(|n| n.len()).max()?;
    let text: Vec<char> = text
        .iter()
        .take(longest)
        .flat_map(|c| c.to_lowercase())
        .collect();
    let (mut best, mut count) = (None, 0);
    for (n, name) in names.iter().enumerate() {
        let i = name
            .chars()
            .zip(&text)
            .take_while(|(a, b)| a.to_ascii_lowercase() == **b)
            .count();
        if i > count || (i == count && i == name.len()) {
            (best, count) = (Some(n), i);
            if i == name.len() && i == text.len() {
                break;
            }
        }
    }
    let best = best.filter(|n| count > 0 && names[*n].len() == count)?;
    Some((best as i32 + 1, count))
}

/// What the text of a `t` field names.
enum Named {
    Local,
    /// Seconds ahead of UTC.
    Ahead(i32),
    Zone(std::sync::Arc<Zone>),
}

/// An offset from UTC as the `t` field of Qt 6.12.0 takes one that is all of `text`, in
/// seconds: `±`, `UTC±` or `GMT±` and hours, with minutes and seconds if any.
/// - `h`, `hh`, `hhmm`, `h:mm`, `h:mm:ss`, and without `GMT` also `hhmmss`. Minutes and
///   seconds are two digits and may be up to 99, each counted in full, but only below 60
///   in `GMT±hhmm`. The whole is at most 16 hours; after `GMT` with colons only the hours
///   are held to that.
/// - After `UTC` or `GMT`, hours, one punctuation character and minutes below 60, at most
///   16 hours in all.
fn zone_offset(text: &[char]) -> Option<i32> {
    const MOST: i32 = 16 * 3600;
    let (gmt, named, rest) = match text {
        ['U', 'T', 'C', rest @ ..] => (false, true, rest),
        ['G', 'M', 'T', rest @ ..] => (true, true, rest),
        rest => (false, false, rest),
    };
    let sign = match rest.first()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let number = |digits: &[char], most: usize| {
        let all = (1..=most).contains(&digits.len()) && digits.iter().all(char::is_ascii_digit);
        all.then(|| digits.iter().collect::<String>().parse::<i32>().ok())?
    };
    let two = |digits: &[char]| number(digits, 2).filter(|_| digits.len() == 2);
    let fields: Vec<&[char]> = rest[1..].split(|c| *c == ':').collect();
    let secs = match fields[..] {
        [all] if all.iter().all(char::is_ascii_digit) => match all.len() {
            1 | 2 => number(all, 2)? * 3600,
            4 => {
                let minutes = two(&all[2..]).filter(|m| !gmt || *m < 60)?;
                two(&all[..2])? * 3600 + minutes * 60
            }
            6 if !gmt => two(&all[..2])? * 3600 + two(&all[2..4])? * 60 + two(&all[4..])?,
            _ => return None,
        },
        // Hours, a punctuation character and minutes.
        [all] => {
            let at = all.iter().position(|c| !c.is_ascii_digit())?;
            let punctuation = all[at].is_ascii_punctuation() && !"$+<=>^`|~".contains(all[at]);
            let minutes = two(&all[at + 1..]).filter(|m| *m < 60 && named && punctuation)?;
            number(&all[..at], 2)? * 3600 + minutes * 60
        }
        [h, m] => number(h, 2)? * 3600 + two(m)? * 60,
        [h, m, s] => number(h, 2)? * 3600 + two(m)? * 60 + two(s)?,
        _ => return None,
    };
    let within = match gmt && fields.len() > 1 {
        true => number(fields[0], 2)? <= 16,
        false => secs <= MOST,
    };
    within.then_some(sign * secs)
}

/// The zone that all of `text` names for a `t` field, as Qt 6.12.0 reads it. In turn: `Z`;
/// an offset from UTC; a name of the local zone, which `LMT` always is; an id QTimeZone
/// knows, of three or more ASCII letters, digits and `+-./:_`, with at most five slashes
/// and 20 characters between them; `GMT`; a zone's long name. An id that is `UTC` and an
/// offset is that offset.
fn zone_named(text: &[char]) -> Option<Named> {
    if text == ['Z'] {
        return Some(Named::Ahead(0));
    }
    if let Some(ahead) = zone_offset(text) {
        return Some(Named::Ahead(ahead));
    }
    let database = Database::system();
    let whole: String = text.iter().collect();
    // Without the local zone's data its names are the C library's.
    let is_local = match database.local() {
        Some(zone) => zone.local_names().contains(&whole.as_str()),
        None => crate::osutil::zone_names().contains(&whole),
    };
    if (is_local && !whole.is_empty()) || whole == "LMT" {
        return Some(Named::Local);
    }
    let plausible = |c: &char| c.is_ascii_alphanumeric() || "+-./:_".contains(*c);
    let is_id = text.len() >= 3
        && text.iter().all(plausible)
        && whole.split('/').count() <= 6
        && whole.split('/').all(|part| part.len() <= 20);
    if let Some(zone) = is_id.then(|| database.zone(&whole)).flatten() {
        let offset = whole.strip_prefix("UTC").unwrap_or(&whole);
        let fixed = whole == "UTC" || offset.starts_with(['+', '-']);
        return Some(match fixed {
            true => Named::Ahead(zone.ahead_at(0)),
            false => Named::Zone(zone),
        });
    }
    // QTimeZonePrivate::findOffsetPrefix: the name of no offset at all.
    if whole == "GMT" {
        return Some(Named::Ahead(0));
    }
    let (zone, len) = database.by_long_name(text)?;
    (len == text.len()).then_some(Named::Zone(zone))
}

/// QGregorianCalendar::matchCenturyToWeekday: the date with that year in its century, month
/// and day that falls on `weekday` (1 is Monday), in one of the four centuries around it.
/// February 29 of a year ending in 00 is in the nearest year that has one.
fn century_on_weekday(year: i32, month: i32, day: i32, weekday: i32) -> Option<NaiveDate> {
    use chrono::Datelike;
    let on = |d: &NaiveDate| d.weekday().number_from_monday() as i32;
    let astronomical = if year < 0 { year + 1 } else { year };
    let found = |year: i32| {
        qdate(if year > 0 { year } else { year - 1 }, month, day).filter(|d| on(d) == weekday)
    };
    let Some(date) = qdate(year, month, day) else {
        let century = astronomical.div_euclid(100).rem_euclid(4);
        if (month, day) != (2, 29) || astronomical.rem_euclid(100) != 0 {
            return None;
        }
        return found(match century {
            1 => astronomical - 100,
            c => astronomical + (4 - c) * 100,
        });
    };
    let diff = on(&date) - weekday;
    if diff == 0 {
        return Some(date);
    }
    let century = (astronomical - i32::from(month < 3)).div_euclid(100);
    if (diff * 4 + century.rem_euclid(4)).rem_euclid(7) >= 4 {
        return None;
    }
    found(astronomical + (((diff.rem_euclid(7) + 3) / 2) % 4 - 1) * 100)
}

impl QDateTime {
    /// QDateTime::fromString(text, format) in the C locale (QDateTimeParser's scanString),
    /// for formats made of `yyyy yy M MM MMM MMMM d dd ddd h hh mm ss A t`: every field must
    /// be spelled in full and agree with the others, and nothing may follow the last one. A
    /// zone is all the text that is left when its field comes (every format V4 has one in
    /// ends with it) and is the result's, as what it is ahead of UTC at that date and
    /// time; without one it is local time.
    pub fn from_format(text: &str, format: &str) -> Option<QDateTime> {
        use chrono::Datelike;
        let (fields, seps) = fields(format);
        let text: Vec<char> = text.chars().collect();
        let twelve = fields.contains(&Field::AmPm);
        let (mut year, mut year2, mut month, mut day) = (1900, None, 1, 1);
        let (mut hour, mut minute, mut second) = (0, 0, 0);
        let (mut weekday, mut pm, mut zone) = (None, 0, Named::Local);
        let mut pos = 0;
        for (field, sep) in fields.iter().zip(&seps) {
            pos += separator(&text[pos..], sep)?;
            let rest = &text[pos..];
            let (value, used) = match *field {
                Field::Year if rest.first() == Some(&'-') => {
                    let (y, n) = number(&rest[1..], 4, 9999, true, false)?;
                    (-y, n + 1)
                }
                Field::Year => number(rest, 4, 9999, true, false)?,
                Field::Year2 => number(rest, 2, 9999, true, false)?,
                Field::Month(n @ (1 | 2)) => number(rest, 2, 12, n == 2, false)?,
                Field::Month(3) => name(rest, &MONTHS)?,
                Field::Month(_) => name(rest, &LONG_MONTHS)?,
                Field::Day(n) => number(rest, 2, 31, n == 2, false)?,
                Field::Weekday => name(rest, &DAYS)?,
                Field::Hour(n) => number(rest, 2, 23, n == 2, twelve)?,
                Field::Minute | Field::Second => number(rest, 2, 59, true, false)?,
                Field::AmPm => {
                    let word: String = rest.iter().take(2).collect();
                    let pm = ["AM", "PM"]
                        .iter()
                        .position(|w| w.eq_ignore_ascii_case(&word))?;
                    (pm as i32, 2)
                }
                Field::Zone => {
                    zone = zone_named(rest)?;
                    (0, rest.len())
                }
            };
            match *field {
                Field::Year if value == 0 => return None,
                Field::Year => year = value,
                Field::Year2 => year2 = Some(value),
                Field::Month(_) if value < 1 => return None,
                Field::Month(_) => month = value,
                Field::Day(_) if value < 1 => return None,
                Field::Day(_) => day = value,
                Field::Weekday => weekday = Some(value),
                Field::Hour(_) => hour = value as u32,
                Field::Minute => minute = value as u32,
                Field::Second => second = value as u32,
                Field::AmPm => pm = value as u32,
                Field::Zone => {}
            }
            pos += used;
        }
        if separator(&text[pos..], seps.last()?)? + pos != text.len() {
            return None;
        }
        // A two-digit year is in the 1900s, unless only another century puts the date on
        // the weekday the text names.
        let date = match (year2, weekday) {
            (Some(y), Some(w)) => century_on_weekday(1900 + y, month, day, w)
                .or_else(|| qdate(1900 + y, month, day))?,
            (Some(y), None) => qdate(1900 + y, month, day)?,
            (None, _) => qdate(year, month, day)?,
        };
        if weekday.is_some_and(|w| w != date.weekday().number_from_monday() as i32) {
            return None;
        }
        let time = NaiveTime::from_hms_opt(hour + 12 * pm, minute, second)?;
        let spec = match zone {
            Named::Local => Spec::Local,
            Named::Ahead(ahead) => Spec::ahead(ahead),
            // QDateTime(date, time, zone): what the zone is ahead of UTC at that instant.
            Named::Zone(zone) => {
                let wall = date.and_time(time).and_utc().timestamp();
                Spec::ahead((wall - zone.instant_of(wall)) as i32)
            }
        };
        Some(QDateTime { date, time, spec })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(
        (y, mo, d): (i32, u32, u32),
        (h, mi, s, ms): (u32, u32, u32, u32),
        spec: Spec,
    ) -> QDateTime {
        QDateTime {
            date: NaiveDate::from_ymd_opt(y, mo, d).unwrap(),
            time: NaiveTime::from_hms_milli_opt(h, mi, s, ms).unwrap(),
            spec,
        }
    }

    #[test]
    fn iso_texts() {
        let iso = QDateTime::from_iso;
        assert_eq!(
            iso("2024-03-10"),
            Some(dt((2024, 3, 10), (0, 0, 0, 0), Spec::Local))
        );
        assert_eq!(
            iso("2024/03/10 10:00"),
            Some(dt((2024, 3, 10), (10, 0, 0, 0), Spec::Local))
        );
        assert_eq!(
            iso("2024-03-10T10:20:30.5Z"),
            Some(dt((2024, 3, 10), (10, 20, 30, 500), Spec::Utc))
        );
        assert_eq!(
            iso("2024-03-10T10:20:30+05:30"),
            Some(dt((2024, 3, 10), (10, 20, 30, 0), Spec::Offset(19800)))
        );
        assert_eq!(iso("2024-03-10T10:20:30+00:00").unwrap().spec, Spec::Utc);
        assert_eq!(
            iso("2024-03-10T24:00:00"),
            Some(dt((2024, 3, 11), (0, 0, 0, 0), Spec::Local))
        );
        assert_eq!(
            iso("2024-03-10T10.5"),
            Some(dt((2024, 3, 10), (10, 30, 0, 0), Spec::Local))
        );
        for bad in [
            "2024-3-10",
            "2024-03-10X10:00",
            "2024-03-10T",
            "2024-02-30",
            "2024-03-10T25:00",
        ] {
            assert_eq!(iso(bad), None, "{bad}");
        }
        assert_eq!(time_from_iso("10:20"), NaiveTime::from_hms_opt(10, 20, 0));
        assert_eq!(
            time_from_iso("10:20:30.5"),
            NaiveTime::from_hms_milli_opt(10, 20, 30, 500)
        );
        assert_eq!(time_from_iso("1:20"), None);
    }

    #[test]
    fn text_and_rfc_texts() {
        let text = QDateTime::from_text;
        let local = dt((2024, 3, 10), (13, 0, 0, 0), Spec::Local);
        assert_eq!(text("Sun Mar 10 13:00:00 2024"), Some(local));
        assert_eq!(text("Sun Mar 10 2024 13:00:00"), Some(local));
        assert_eq!(
            text("Sun Mar 10 2024 13:00:00 GMT+0300").unwrap().spec,
            Spec::Offset(10800)
        );
        assert_eq!(
            text("Sun Mar 10 13:00:00 2024 UTC").unwrap().spec,
            Spec::Utc
        );
        assert_eq!(text("Sun Mar 10 2024"), None);
        assert_eq!(text("Sun mar 10 13:00:00 2024"), None);
        let rfc = QDateTime::from_rfc2822;
        assert_eq!(
            rfc("10 Mar 2024 10:00:00"),
            Some(dt((2024, 3, 10), (10, 0, 0, 0), Spec::Utc))
        );
        assert_eq!(
            rfc("Sun, 10 Mar 2024 10:00:00 +0100"),
            Some(dt((2024, 3, 10), (10, 0, 0, 0), Spec::Offset(3600)))
        );
        assert_eq!(rfc("Mon, 10 Mar 2024 10:00:00 +0100"), None);
        assert_eq!(rfc("10 Mar 2024 10:00:00 GMT"), None);
        assert_eq!(rfc("10 Mar 2024"), None);
    }
}
