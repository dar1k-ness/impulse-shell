//! Time zones as qtbase 6.12's QTimeZone has them on Linux (qtimezone.cpp,
//! qtimezoneprivate.cpp, qtimezoneprivate_tz.cpp): the zones of the time zone database, read
//! from its TZif files (RFC 9636); POSIX `TZ` rules; `UTC±hh[:mm[:ss]]`; and the zones' long
//! names. A zone keeps what its clocks are ahead of UTC and the abbreviations its data has.

use std::collections::{BTreeSet, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use chrono::{Datelike, NaiveDate};

use crate::locale::Locale;
use crate::locale_data::ZONES;
use crate::zone_aliases::ALIASES;

/// What the system's zone is ahead of UTC, in seconds, at `secs` since the epoch: by the
/// system's zone data, read where Qt and the C library read it (`TZDIR` first); UTC when
/// there is none to read. Every local clock reads this, as each of Qt's reads the one
/// system zone, and sees a change of the zone on its next read.
pub fn local_ahead(secs: i64) -> i32 {
    let zone = Database::system().local();
    zone.map_or(0, |zone| zone.ahead_at(secs))
}

/// The system zone's standard time now, in seconds ahead of UTC (see
/// [`Zone::standard_ahead_at`]); 0 without a zone to read.
pub fn local_standard_ahead() -> i32 {
    let now = chrono::Utc::now().timestamp();
    let zone = Database::system().local();
    zone.map_or(0, |zone| zone.standard_ahead_at(now))
}

/// A day of the year in a POSIX rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Day {
    /// `Mm.w.d`: the `week`th `weekday` (1 is Monday, 7 Sunday) of `month`; the fifth is
    /// the last.
    Nth { month: u32, week: u32, weekday: u32 },
    /// `Jn`: the nth day of the year, 1 to 365, where February 29 is never counted.
    Julian(u32),
    /// `n`: days after January 1, 0 to 365, where February 29 is counted.
    After(u32),
}

/// When a POSIX rule changes the clocks: a day and seconds after its midnight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Turn {
    day: Day,
    secs: i32,
}

/// The daylight-saving part of a POSIX rule: seconds ahead of UTC, and when it starts and
/// ends each year.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Saving {
    ahead: i32,
    starts: Turn,
    ends: Turn,
}

/// A POSIX `TZ` rule, `std offset [dst [offset],start[/time],end[/time]]`: standard time's
/// seconds ahead of UTC and the daylight-saving time, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rule {
    std: i32,
    saving: Option<Saving>,
}

/// When a zone's clocks changed, in seconds since the epoch, and the index of the [`Kind`]
/// of local time that holds from then on.
type Change = (i64, usize);

/// A kind of local time: seconds ahead of UTC and whether it is daylight-saving time.
type Kind = (i32, bool);

/// A zone: what its clocks are ahead of UTC through time.
#[derive(Debug, PartialEq, Eq)]
pub struct Zone {
    /// In the file's order.
    changes: Vec<Change>,
    /// The first also holds before the first change.
    kinds: Vec<Kind>,
    /// The abbreviation of each of `kinds`, for a zone read from a file.
    abbreviations: Vec<String>,
    /// What holds after the last change.
    rule: Option<Rule>,
    /// The names `rule`'s text gives its standard and its daylight-saving time.
    rule_names: [String; 2],
}

/// qstrntoll's decimal number at the start of `s`, with its sign if any: the value and how
/// many bytes it takes.
fn number(s: &[u8]) -> Option<(i64, usize)> {
    let sign = usize::from(matches!(s.first(), Some(b'+' | b'-')));
    let digits = s[sign..].iter().take_while(|c| c.is_ascii_digit()).count();
    let text = std::str::from_utf8(&s[..sign + digits]).ok()?;
    Some((text.parse().ok()?, sign + digits))
}

/// QLatin1StringView::toInt of the whole of `s`.
fn int(s: &[u8]) -> Option<i64> {
    number(s).filter(|n| n.1 == s.len()).map(|n| n.0)
}

/// parsePosixTime: `h[h][:m[m][:s[s]]]` in seconds. Each number is at most two bytes, sign
/// included; an hour below zero still has its minutes and seconds added.
fn posix_time(s: &[u8]) -> Option<i32> {
    let field = |s: &[u8], least: i64, most: i64| {
        number(s).filter(|(n, used)| *used <= 2 && (least..=most).contains(n))
    };
    let (hour, used) = field(s, -137, 137)?;
    let (mut minute, mut second, mut rest) = (0, 0, &s[used..]);
    if let Some(after) = rest.strip_prefix(b":") {
        let (m, used) = field(after, 0, 59)?;
        (minute, rest) = (m, &after[used..]);
        if let Some(after) = rest.strip_prefix(b":") {
            let (s, used) = field(after, 0, 59)?;
            (second, rest) = (s, &after[used..]);
        }
    }
    rest.is_empty()
        .then_some(((hour * 60 + minute) * 60 + second) as i32)
}

/// parsePosixOffset: `[+|-]` and a time, as seconds ahead of UTC. POSIX counts hours behind
/// UTC, so no sign and `+` are behind it.
fn posix_offset(s: &[u8]) -> Option<i32> {
    match s.first()? {
        b'+' => posix_time(&s[1..]).map(|t| -t),
        b'-' => posix_time(&s[1..]),
        _ => posix_time(s).map(|t| -t),
    }
}

/// PosixZone::parse at `pos`, which moves past what was read: a name of three or more
/// letters, or of anything between `<` and `>`, then an offset. None without such a name,
/// or for a UTC or GMT that is not at offset 0, which only the C library takes (`libc`);
/// otherwise the name and the offset, if a valid one is there.
fn posix_zone<'a>(s: &'a [u8], pos: &mut usize, libc: bool) -> Option<(&'a [u8], Option<i32>)> {
    let name = if s.get(*pos) == Some(&b'<') {
        let len = s[*pos + 1..].iter().position(|c| *c == b'>')?;
        *pos += len + 2;
        &s[*pos - len - 1..*pos - 1]
    } else {
        let len = s[*pos..]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .count();
        *pos += len;
        &s[*pos - len..*pos]
    };
    if name.len() < 3 {
        return None;
    }
    let sign = usize::from(matches!(s.get(*pos), Some(b'+' | b'-')));
    let len = sign
        + s[*pos + sign..]
            .iter()
            .take_while(|c| c.is_ascii_digit() || **c == b':')
            .count();
    let offset = match len {
        0 => None,
        _ => posix_offset(&s[*pos..*pos + len]),
    };
    *pos += len;
    if !libc && offset != Some(0) && (name == b"UTC" || name == b"GMT") {
        return None;
    }
    Some((name, offset))
}

/// The names calculatePosixTransitions gives the standard and the daylight-saving time of
/// a rule [`Rule::parse_as`] takes; the second is empty when the rule has only one.
fn rule_names(text: &[u8]) -> [String; 2] {
    let zone = text.split(|c| *c == b',').next().unwrap_or_default();
    let (zone, mut pos) = (zone.trim_ascii(), 0);
    [0, 1].map(|_| {
        let name = (pos < zone.len()).then(|| posix_zone(zone, &mut pos, true));
        let name = name.flatten().map(|zone| zone.0).unwrap_or_default();
        String::from_utf8_lossy(name).into_owned()
    })
}

impl Day {
    /// calculatePosixDate's reading of a rule's date; None for one it has no date for.
    fn parse(s: &[u8]) -> Option<Day> {
        match *s.first()? {
            b'M' => {
                let mut parts = s[1..].split(|c| *c == b'.');
                let mut part = || int(parts.next()?);
                let (month, week, weekday) = (part()?, part()?, part()?);
                let valid = (1..=12).contains(&month)
                    && (1..=5).contains(&week)
                    && (0..=7).contains(&weekday);
                valid.then_some(Day::Nth {
                    month: month as u32,
                    week: week as u32,
                    weekday: if weekday == 0 { 7 } else { weekday as u32 },
                })
            }
            b'J' => int(&s[1..])
                .filter(|n| (1..366).contains(n))
                .map(|n| Day::Julian(n as u32)),
            _ => int(s)
                .filter(|n| (0..366).contains(n))
                .map(|n| Day::After(n as u32)),
        }
    }

    /// calculatePosixDate: this day in `year`. A day after December 31 is in the next year.
    fn in_year(self, year: i32) -> Option<NaiveDate> {
        let january = NaiveDate::from_ymd_opt(year, 1, 1)?;
        let leap = january.leap_year();
        let days = |n: i64| chrono::Days::new(n as u64);
        match self {
            Day::Nth {
                month,
                week,
                weekday,
            } => {
                let first = NaiveDate::from_ymd_opt(year, month, 1)?;
                let starts = first.weekday().number_from_monday();
                // The last such weekday before the month, then `week` weeks on.
                let back = i64::from((starts + 7 - weekday - 1) % 7 + 1);
                let mut date = first
                    .checked_sub_days(days(back))?
                    .checked_add_days(days(i64::from(week) * 7))?;
                while date.month() != month {
                    date = date.checked_sub_days(days(7))?;
                }
                Some(date)
            }
            Day::Julian(n) if !leap || n < 60 => january.checked_add_days(days(i64::from(n) - 1)),
            Day::Julian(n) | Day::After(n) => january.checked_add_days(days(i64::from(n))),
        }
    }
}

impl Turn {
    /// `date[/time]`; the time is 02:00 when left out.
    fn parse(s: &[u8]) -> Option<Turn> {
        let mut parts = s.split(|c| *c == b'/');
        let day = Day::parse(parts.next()?)?;
        let secs = match parts.next() {
            Some(time) => posix_time(time)?,
            None => 7200,
        };
        Some(Turn { day, secs })
    }

    /// The local date and time of this turn in `year`, in seconds as if it were UTC.
    fn wall(self, year: i32) -> Option<i64> {
        let midnight = self.day.in_year(year)?.and_time(chrono::NaiveTime::MIN);
        Some(midnight.and_utc().timestamp() + i64::from(self.secs))
    }
}

/// The year of a time in seconds since the epoch.
fn year_of(secs: i64) -> Option<i32> {
    Some(chrono::DateTime::from_timestamp(secs, 0)?.year())
}

impl Rule {
    /// validatePosixRule, then what calculatePosixTransitions takes from the rule. With
    /// `need_offset`, a name without an offset is no rule; otherwise it is UTC. A
    /// daylight-saving name needs both its dates; its offset is an hour ahead of standard
    /// time's when left out.
    fn parse(text: &[u8], need_offset: bool) -> Option<Rule> {
        Rule::parse_as(text, need_offset, false)
    }

    /// [`Rule::parse`], or with `libc` the C library's reading of a value of `TZ`, where
    /// it is not Qt's: UTC and GMT may have an offset, and daylight-saving time without
    /// dates is the United States'.
    fn parse_as(text: &[u8], need_offset: bool, libc: bool) -> Option<Rule> {
        let mut parts = text.split(|c| *c == b',');
        let zone = parts.next()?.trim_ascii();
        let mut dates = [parts.next(), parts.next(), parts.next()];
        if zone.is_empty() {
            return None;
        }
        let mut pos = 0;
        let std = posix_zone(zone, &mut pos, libc)?.1;
        if libc && dates[0].is_none() && pos < zone.len() {
            dates = [Some(b"M3.2.0"), Some(b"M11.1.0"), None];
        }
        if need_offset && std.is_none() {
            return None;
        }
        let saving = match dates {
            [None, ..] => None,
            [Some(starts), Some(ends), None] if pos < zone.len() => {
                let ahead = posix_zone(zone, &mut pos, libc)?.1;
                let (starts, ends) = (Turn::parse(starts)?, Turn::parse(ends)?);
                // Both dates have to exist in a leap year.
                starts.day.in_year(1972).and(ends.day.in_year(1972))?;
                std.map(|std| Saving {
                    ahead: ahead.unwrap_or(std + 3600),
                    starts,
                    ends,
                })
            }
            _ => return None,
        };
        (pos == zone.len()).then_some(Rule {
            std: std.unwrap_or(0),
            saving,
        })
    }

    /// Whether the rule's daylight-saving time holds at `secs`: calculatePosixTransitions
    /// for the year before `secs`'s to the year after it, and QTzTimeZonePrivate::data's
    /// choice among them, the latest that is not after `secs`. `table` is whether the
    /// zone's file has changes of its own (which all are before `secs`); without them the
    /// first of the rule's changes also holds before it. None when the rule has no change
    /// to go by.
    fn saving_at(&self, secs: i64, table: bool) -> Option<bool> {
        let Some(saving) = self.saving else {
            return Some(false);
        };
        let year = year_of(secs)?;
        let mut changes = Vec::with_capacity(6);
        for y in year - 1..=year + 1 {
            let (starts, ends) = (saving.starts.wall(y)?, saving.ends.wall(y)?);
            let to_saving = (starts - i64::from(self.std), true);
            let to_std = (ends - i64::from(saving.ahead), false);
            let january =
                |y| Some(NaiveDate::from_ymd_opt(y, 1, 1)?.and_time(chrono::NaiveTime::MIN));
            let (first, next) = (
                january(y)?.and_utc().timestamp(),
                january(y + 1)?.and_utc().timestamp(),
            );
            if y == year - 1 {
                // One of the two that lasts the whole year is all there is.
                if to_saving.0 < to_std.0 {
                    if starts <= first && ends >= next {
                        return Some(true);
                    }
                } else if ends <= first && starts >= next {
                    return Some(false);
                }
            }
            // A turn whose time puts it in another year is left out.
            let in_year = |wall| year_of(wall) == Some(y);
            match (in_year(ends), in_year(starts)) {
                (true, true) if starts < ends => changes.extend([to_saving, to_std]),
                (true, true) => changes.extend([to_std, to_saving]),
                (true, false) => changes.push(to_std),
                (false, true) => changes.push(to_saving),
                (false, false) => {}
            }
        }
        match changes.partition_point(|c| c.0 <= secs) {
            0 if table => None,
            0 => changes.first().map(|c| c.1),
            n => Some(changes[n - 1].1),
        }
    }
}

/// What is left of a file's bytes while it is read.
struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(taken)
    }

    fn u32(&mut self) -> Option<usize> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?) as usize)
    }
}

/// The counts in a TZif header (RFC 9636 3.1).
struct Counts {
    isutc: usize,
    isstd: usize,
    leap: usize,
    time: usize,
    kind: usize,
    chars: usize,
}

/// parseTzHeader: the version byte and the counts of a header, which has to be of version
/// 1, 2 or 3 and within Qt's limits.
fn header(bytes: &mut Bytes) -> Option<(u8, Counts)> {
    if bytes.take(4)? != b"TZif" {
        return None;
    }
    let version = bytes.take(1)?[0];
    if !matches!(version, 0 | b'2' | b'3') {
        return None;
    }
    bytes.take(15)?;
    let counts = Counts {
        isutc: bytes.u32()?,
        isstd: bytes.u32()?,
        leap: bytes.u32()?,
        time: bytes.u32()?,
        kind: bytes.u32()?,
        chars: bytes.u32()?,
    };
    let within = counts.time <= 1200
        && counts.kind <= 256
        && counts.chars <= 50
        && counts.leap <= 50
        && counts.isutc <= counts.kind
        && counts.isstd <= counts.kind;
    within.then_some((version, counts))
}

/// A header's data block, whose times are `width` bytes each: the changes and the kinds of
/// local time. The abbreviations, leap seconds and indicators are passed over.
fn block(
    bytes: &mut Bytes,
    counts: &Counts,
    width: usize,
) -> Option<(Vec<Change>, Vec<Kind>, Vec<String>)> {
    let times = bytes.take(counts.time * width)?;
    let indices = bytes.take(counts.time)?;
    let kinds = bytes.take(counts.kind * 6)?;
    let chars = bytes.take(counts.chars)?;
    bytes.take(counts.leap * (width + 4) + counts.isstd + counts.isutc)?;
    let time = |t: &[u8]| match width {
        4 => t.try_into().ok().map(|t| i64::from(i32::from_be_bytes(t))),
        _ => t.try_into().ok().map(i64::from_be_bytes),
    };
    let changes = times
        .chunks(width)
        .zip(indices)
        .map(|(t, i)| Some((time(t)?, usize::from(*i))).filter(|c| c.1 < counts.kind))
        .collect::<Option<_>>()?;
    let kinds_parsed = kinds
        .chunks(6)
        .map(|k| Some((i32::from_be_bytes(k[..4].try_into().ok()?), k[4] != 0)))
        .collect::<Option<_>>()?;
    let abbreviation = |k: &[u8]| {
        let name = chars.get(usize::from(k[5])..).unwrap_or_default();
        let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        String::from_utf8_lossy(&name[..end]).into_owned()
    };
    let abbreviations = kinds.chunks(6).map(abbreviation).collect();
    Some((changes, kinds_parsed, abbreviations))
}

impl Zone {
    /// A zone that is always `ahead` seconds ahead of UTC.
    fn fixed(ahead: i32) -> Zone {
        Zone {
            changes: vec![],
            kinds: vec![(ahead, false)],
            abbreviations: vec![],
            rule: None,
            rule_names: Default::default(),
        }
    }

    /// The zone a POSIX rule is, whose `text` names its times.
    fn of_rule(rule: Rule, text: &[u8]) -> Zone {
        Zone {
            changes: vec![],
            kinds: vec![],
            abbreviations: vec![],
            rule: Some(rule),
            rule_names: rule_names(text),
        }
    }

    /// A TZif file as QTzTimeZoneCache::findEntry reads it: the first data block, or from
    /// version 2 on the second one, with 64-bit times, and the POSIX rule in the footer.
    /// None for a file that is cut short, breaks Qt's limits, has a rule Qt does not take,
    /// or has neither a change nor a rule.
    fn from_tzif(data: &[u8]) -> Option<Zone> {
        let mut bytes = Bytes(data);
        let (version, counts) = header(&mut bytes)?;
        let (mut changes, mut kinds, mut abbreviations) = block(&mut bytes, &counts, 4)?;
        let (mut rule, mut rule_names) = (None, Default::default());
        if version != 0 {
            let (_, counts) = header(&mut bytes)?;
            (changes, kinds, abbreviations) = block(&mut bytes, &counts, 8)?;
            if bytes.take(1)? == b"\n" {
                let end = bytes.0.iter().position(|c| *c == b'\n')?;
                if end > 0 {
                    rule = Some(Rule::parse(&bytes.0[..end], false)?);
                    rule_names = self::rule_names(&bytes.0[..end]);
                }
            }
        }
        (!changes.is_empty() || rule.is_some()).then_some(Zone {
            changes,
            kinds,
            abbreviations,
            rule,
            rule_names,
        })
    }

    /// QTimeZone::offsetFromUtc: seconds ahead of UTC at `secs` since the epoch. After the
    /// last change of the file it is the rule's, where there is one; before the first it is
    /// the file's first kind of local time.
    pub fn ahead_at(&self, secs: i64) -> i32 {
        match self.by_rule(secs) {
            Some((rule, saving)) => rule.saving.filter(|_| saving).map_or(rule.std, |s| s.ahead),
            None => self.kinds.get(self.kind_at(secs)).map_or(0, |k| k.0),
        }
    }

    /// The rule, when it is what holds at `secs`, and whether its daylight-saving time
    /// does then ([`Rule::saving_at`]).
    fn by_rule(&self, secs: i64) -> Option<(Rule, bool)> {
        let last = self.changes.last().map(|c| c.0);
        let rule = self.rule.filter(|_| last.is_none_or(|last| last < secs))?;
        Some((rule, rule.saving_at(secs, last.is_some())?))
    }

    /// The index of the file's kind of local time at `secs`.
    fn kind_at(&self, secs: i64) -> usize {
        match self.changes.partition_point(|c| c.0 <= secs) {
            0 => 0,
            n => self.changes[n - 1].1,
        }
    }

    /// QTimeZone::abbreviation and isDaylightTime at `secs` since the epoch
    /// (QTzTimeZonePrivate::data): where [`Zone::ahead_at`] goes by the rule, the name the
    /// rule gives that time, and daylight-saving time is one that is not at standard time's
    /// offset; else what the file says of its kind of local time. A zone that is no file's
    /// and no rule's has no abbreviation.
    pub fn name_at(&self, secs: i64) -> (&str, bool) {
        if let Some((rule, saving)) = self.by_rule(secs) {
            let daylight = saving && rule.saving.is_some_and(|s| s.ahead != rule.std);
            return (&self.rule_names[usize::from(saving)], daylight);
        }
        let kind = self.kind_at(secs);
        let name = self.abbreviations.get(kind).map_or("", String::as_str);
        (name, self.kinds.get(kind).is_some_and(|k| k.1))
    }

    /// The zone's standard time at `secs` since the epoch, in seconds ahead of UTC: what
    /// its clocks would be ahead by without daylight-saving time (the C library's
    /// `timezone` at that moment, which QLocalTime::getCurrentStandardUtcOffset reads).
    pub fn standard_ahead_at(&self, secs: i64) -> i32 {
        let last = self.changes.last().map(|c| c.0);
        if let Some(rule) = self.rule.filter(|_| last.is_none_or(|last| last < secs)) {
            return rule.std;
        }
        let upto = self.changes.partition_point(|c| c.0 <= secs);
        let kinds = self.changes[..upto].iter().rev().map(|c| c.1);
        let standard = kinds
            .chain([0])
            .find(|kind| self.kinds.get(*kind).is_some_and(|k| !k.1));
        match standard.and_then(|kind| self.kinds.get(kind)) {
            Some(kind) => kind.0,
            None => self.ahead_at(secs),
        }
    }

    /// The instant QDateTime(date, time, zone) is: seconds since the epoch when the zone's
    /// clocks show `wall`, a local date and time in seconds as if it were UTC
    /// (QTimeZonePrivate::stateAtZoneTime, resolving as RelativeToBefore). A time the clocks
    /// show twice is the first of the two; one they skip is read with the offset from
    /// before the skip.
    pub fn instant_of(&self, wall: i64) -> i64 {
        const AROUND: i64 = 17 * 3600;
        let (before, after) = (self.ahead_at(wall - AROUND), self.ahead_at(wall + AROUND));
        let (by_before, by_after) = (wall - i64::from(before), wall - i64::from(after));
        let only_after = before != after
            && self.ahead_at(by_after) == after
            && self.ahead_at(by_before) != before;
        if only_after { by_after } else { by_before }
    }

    /// The names QDateTime's parser reads as this zone when it is the local one, whatever
    /// the date: its abbreviation before its first change, the one at the start of 1900
    /// and that of the first daylight-saving time from then on (the C library's `tzname`
    /// for that time), and those of its last standard and last daylight-saving time.
    pub fn local_names(&self) -> Vec<&str> {
        const YEAR_1900: i64 = -2_208_988_800;
        let from_1900 = self.changes.partition_point(|c| c.0 <= YEAR_1900);
        let then = from_1900.checked_sub(1).map_or(0, |i| self.changes[i].1);
        let daylight = |c: &&Change| self.kinds[c.1].1;
        let standard = |c: &&Change| !self.kinds[c.1].1;
        let changes = [
            self.changes[from_1900..].iter().find(daylight),
            self.changes.iter().rev().find(standard),
            self.changes.iter().rev().find(daylight),
        ];
        let kinds = [0, then]
            .into_iter()
            .chain(changes.into_iter().flatten().map(|c| c.1));
        kinds
            .filter_map(|kind| self.abbreviations.get(kind).map(String::as_str))
            .collect()
    }

    /// QTimeZone::hasDaylightTime: the rule has daylight-saving time, or a change of the
    /// file is to it.
    fn has_daylight_time(&self) -> bool {
        self.rule.is_some_and(|r| r.saving.is_some())
            || self.changes.iter().any(|c| self.kinds[c.1].1)
    }
}

/// The id, which is also every name, of QTimeZone(offsetSeconds) for an offset that no
/// zone of QUtcTimeZonePrivate's table has (QTimeZonePrivate::isoOffsetFormat):
/// `UTC±hh:mm`, and `:ss` when the offset has seconds.
pub fn offset_id(ahead: i32) -> String {
    let (sign, ahead) = if ahead < 0 {
        ('-', -ahead)
    } else {
        ('+', ahead)
    };
    let id = format!("UTC{sign}{:02}:{:02}", ahead / 3600, ahead / 60 % 60);
    match ahead % 60 {
        0 => id,
        secs => format!("{id}:{secs:02}"),
    }
}

/// QUtcTimeZonePrivate::offsetFromUtcString: `UTC±h[h][:mm[:ss]]` in seconds, under a day.
fn utc_offset(id: &str) -> Option<i32> {
    let rest = id.strip_prefix("UTC").filter(|rest| rest.len() >= 2)?;
    let sign = match rest.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let mut fields = rest[1..].split(':');
    let mut field = |first: bool| match fields.next() {
        Some(f) if f.len() > 2 || (!first && f.len() < 2) => None,
        Some(f) => {
            let most = if first { 23 } else { 59 };
            f.parse::<u16>().ok().filter(|n| *n <= most).map(i32::from)
        }
        None => Some(0),
    };
    let secs = (field(true)? * 60 + field(false)?) * 60 + field(false)?;
    fields.next().is_none().then_some(secs * sign)
}

/// QTimeZonePrivate::aliasToIana: the id CLDR has `id` under, when that is another one.
fn alias_of(id: &str) -> Option<&'static str> {
    ALIASES.iter().find(|a| a.0 == id).map(|a| a.1)
}

/// The ids QUtcTimeZonePrivate has in its table: `UTC` and the whole offsets in use.
fn utc_ids() -> impl Iterator<Item = &'static str> {
    ZONES.iter().map(|z| z.0).filter(|id| id.starts_with("UTC"))
}

/// std::partition as libstdc++ does it: those `first` is true for come first, the rest in
/// the order its swaps leave them in.
fn partition<T>(all: &mut [T], first: impl Fn(&T) -> bool) {
    let (mut low, mut high) = (0, all.len());
    loop {
        while low < high && first(&all[low]) {
            low += 1;
        }
        if low == high {
            return;
        }
        high -= 1;
        while low < high && !first(&all[high]) {
            high -= 1;
        }
        if low == high {
            return;
        }
        all.swap(low, high);
        low += 1;
    }
}

/// The time zone database: the directories its files are looked for in, and the zones Qt
/// lists from them.
pub struct Database {
    dirs: Vec<PathBuf>,
    /// The ids of zone.tab and the TZif files beside it (tzZones).
    ids: BTreeSet<String>,
    /// The zones of the files read so far, by id; None for a file that is no zone.
    zones: Mutex<HashMap<String, Option<Arc<Zone>>>>,
    names: OnceLock<Names>,
    /// The file the system's zone is in when `TZ` names none: /etc/localtime.
    localtime: PathBuf,
    /// The system's zone as last read, and what it was read from.
    local: Mutex<Option<(Source, Option<Arc<Zone>>)>>,
}

/// What the system's zone was read from, by which the C library's tzset tells that it has
/// to read the zone again.
#[derive(Debug, PartialEq, Eq)]
enum Source {
    /// The zone `TZ` names.
    Named(String),
    /// /etc/localtime, with its device, inode and time of modification in seconds and
    /// nanoseconds; None when there is no such file.
    File(Option<[i64; 4]>),
}

/// A zone's standard and daylight-saving long names, each in lower case and with its
/// length in characters.
type LongNames = [Option<(Vec<char>, usize)>; 2];

/// The zones a long name is looked for among: each id with its standard and
/// daylight-saving long names in the C locale, in lower case and with their lengths in
/// characters. First the ids QTimeZonePrivate::getCanonicalZonesThenAliases goes through,
/// `listed` of them, then CLDR's aliases that are not among those.
struct Names {
    zones: Vec<(String, LongNames)>,
    listed: usize,
}

impl Database {
    /// The system's database, as it is when first asked for: in `TZDIR`, then
    /// /usr/share/zoneinfo, then /usr/lib/zoneinfo.
    pub fn system() -> &'static Database {
        static SYSTEM: OnceLock<Database> = OnceLock::new();
        SYSTEM.get_or_init(|| {
            let tzdir = std::env::var_os("TZDIR").filter(|d| !d.is_empty());
            let dirs = tzdir.into_iter().map(PathBuf::from);
            Database::in_dirs(
                dirs.chain(["/usr/share/zoneinfo".into(), "/usr/lib/zoneinfo".into()])
                    .collect(),
            )
        })
    }

    /// The database in `dirs`. Its zones are loadTzTimeZones's: those of the first zone.tab
    /// and the TZif files of that file's directory outside posix/ and right/. Without a
    /// zone.tab there are none.
    fn in_dirs(dirs: Vec<PathBuf>) -> Database {
        let mut database = Database {
            dirs,
            ids: BTreeSet::new(),
            zones: Mutex::default(),
            names: OnceLock::new(),
            localtime: "/etc/localtime".into(),
            local: Mutex::default(),
        };
        if let Some((table, dir)) = database.read("zone.tab") {
            // Territory, coordinates, id and comment, with tabs between.
            let ids = table.split(|c| *c == b'\n').filter_map(|line| {
                let mut columns = line.trim_ascii().split(|c| *c == b'\t');
                columns.next().filter(|t| !t.is_empty() && t[0] != b'#')?;
                columns.next()?;
                let id = String::from_utf8(columns.next()?.to_vec()).ok()?;
                Some(id).filter(|id| !id.is_empty())
            });
            database.ids.extend(ids);
            list_tzif(&dir, "", &mut database.ids);
        }
        database
    }

    /// openZoneInfo: the content of the file `name` in the first directory that has one to
    /// read, and that directory.
    fn read(&self, name: &str) -> Option<(Vec<u8>, PathBuf)> {
        self.dirs
            .iter()
            .find_map(|dir| Some((std::fs::read(dir.join(name)).ok()?, dir.clone())))
    }

    /// QTzTimeZonePrivate(id): the zone of a listed id's file, or of an id that is a POSIX
    /// rule with an offset. An id whose file cannot be read is taken as a rule.
    fn of_database(&self, id: &str) -> Option<Arc<Zone>> {
        if !self.is_available(id) {
            return None;
        }
        let known = |zones: &HashMap<String, Option<Arc<Zone>>>| zones.get(id).cloned();
        if let Some(zone) = self.zones.lock().ok().as_deref().and_then(known) {
            return zone;
        }
        let Some((data, _)) = self.read(id) else {
            let rule = Rule::parse(id.as_bytes(), false)?;
            return Some(Arc::new(Zone::of_rule(rule, id.as_bytes())));
        };
        let zone = Zone::from_tzif(&data).map(Arc::new);
        if let Ok(mut zones) = self.zones.lock() {
            zones.insert(id.to_string(), zone.clone());
        }
        zone
    }

    /// QTzTimeZonePrivate::isTimeZoneIdAvailable.
    fn is_available(&self, id: &str) -> bool {
        self.ids.contains(id) || Rule::parse(id.as_bytes(), true).is_some()
    }

    /// The system's zone, as the C library finds it, which Qt's local time is read from
    /// (qLocalTime: tzset, then localtime_r): the zone `TZ` names, else the one
    /// /etc/localtime holds. None when there is no zone to read, which is UTC.
    ///
    /// The zone is read again when it may have changed, as tzset tells: `TZ` has another
    /// value, or without `TZ` /etc/localtime is another file or has been written to. So a
    /// read costs no system call with `TZ` and one `stat` without it.
    pub fn local(&self) -> Option<Arc<Zone>> {
        let tz = std::env::var_os("TZ");
        self.local_by(tz.as_deref().map(|tz| tz.to_string_lossy()).as_deref())
    }

    /// [`Database::local`] where `TZ` is `tz`.
    fn local_by(&self, tz: Option<&str>) -> Option<Arc<Zone>> {
        // tzset skips a leading colon.
        let name = tz.map(|tz| tz.strip_prefix(':').unwrap_or(tz));
        let mut last = self.local.lock().unwrap_or_else(|e| e.into_inner());
        let source = match name {
            Some(name) => match &*last {
                Some((Source::Named(read), zone)) if read == name => return zone.clone(),
                _ => Source::Named(name.into()),
            },
            None => Source::File(std::fs::metadata(&self.localtime).ok().map(|file| {
                use std::os::unix::fs::MetadataExt;
                let (device, inode) = (file.dev() as i64, file.ino() as i64);
                [device, inode, file.mtime(), file.mtime_nsec()]
            })),
        };
        if let Some((read, zone)) = &*last
            && *read == source
        {
            return zone.clone();
        }
        let zone = match &source {
            Source::Named(name) => self.named_by_tz(name),
            Source::File(_) => self.in_file(&self.localtime),
        };
        *last = Some((source, zone.clone()));
        zone
    }

    /// The zone in the TZif file at `path`.
    fn in_file(&self, path: &Path) -> Option<Arc<Zone>> {
        let data = std::fs::read(path).ok()?;
        Zone::from_tzif(&data).map(Arc::new)
    }

    /// The zone a value `name` of `TZ` stands for, as the C library reads it (tzset): UTC
    /// for an empty one; else the zone of the file at that path, for a relative one in the
    /// first directory of the database only (`TZDIR`, else /usr/share/zoneinfo); else the
    /// POSIX rule it is; else none.
    fn named_by_tz(&self, name: &str) -> Option<Arc<Zone>> {
        if name.is_empty() {
            return Some(Arc::new(Zone::fixed(0)));
        }
        let file = self.dirs.first().map(|dir| dir.join(name));
        if let Some(zone) = file.and_then(|file| self.in_file(&file)) {
            return Some(zone);
        }
        let rule = Rule::parse_as(name.as_bytes(), true, true)?;
        Some(Arc::new(Zone::of_rule(rule, name.as_bytes())))
    }

    /// QTimeZone(id): an id of QUtcTimeZonePrivate's table; else the database's zone or a
    /// POSIX rule; else the zone of the id CLDR has it under, or of an available id CLDR
    /// has under it; else `UTC±h[h][:mm[:ss]]`.
    pub fn zone(&self, id: &str) -> Option<Arc<Zone>> {
        if utc_ids().any(|utc| utc == id) {
            return Some(Arc::new(Zone::fixed(utc_offset(id).unwrap_or(0))));
        }
        self.in_database(id)
            .or_else(|| utc_offset(id).map(|ahead| Arc::new(Zone::fixed(ahead))))
    }

    /// The database's zone `id` or the POSIX rule it is; else the zone of the id CLDR has it
    /// under, or of an available id CLDR has under it.
    fn in_database(&self, id: &str) -> Option<Arc<Zone>> {
        let by_alias = || {
            let other = alias_of(id).or_else(|| {
                let aliases = ALIASES.iter().filter(|a| a.1 == id).map(|a| a.0);
                let available = |alias: &&str| self.is_available(alias);
                aliases
                    .into_iter()
                    .find(available)
                    .filter(|_| !self.is_available(id))
            })?;
            self.of_database(other)
        };
        self.of_database(id).or_else(by_alias)
    }

    /// The id QTimeZone(id) has where it is an offset from UTC and no zone of the database:
    /// `id` itself where QUtcTimeZonePrivate's table has it, else the id of its offset
    /// ([`offset_id`]: `UTC+3` is `UTC+03:00`).
    pub fn utc_id(&self, id: &str) -> Option<String> {
        if utc_ids().any(|utc| utc == id) {
            return Some(id.into());
        }
        match self.in_database(id) {
            Some(_) => None,
            None => utc_offset(id).map(offset_id),
        }
    }

    /// The zones [`Database::by_long_name`] goes through. Those of
    /// getCanonicalZonesThenAliases are the ids of QUtcTimeZonePrivate's table and of the
    /// database with the ids CLDR has those under, sorted, then those that are no alias
    /// moved before those that are.
    fn names(&self) -> &Names {
        self.names.get_or_init(|| {
            let mut all: BTreeSet<&str> = self.ids.iter().map(String::as_str).collect();
            let under: Vec<&str> = all.iter().filter_map(|id| alias_of(id)).collect();
            all.extend(under);
            for id in utc_ids() {
                all.insert(id);
            }
            let unlisted = ALIASES.iter().map(|a| a.0).filter(|a| !all.contains(a));
            let mut ids: Vec<&str> = all.iter().copied().collect();
            partition(&mut ids, |id| alias_of(id).is_none());
            let listed = ids.len();
            let named = |id: &str| {
                let names = [false, true].map(|daylight| {
                    let name = Locale::c().zone_name(id, daylight, true)?;
                    let lower = name.chars().flat_map(char::to_lowercase).collect();
                    Some((lower, name.chars().count()))
                });
                (id.to_string(), names)
            };
            Names {
                zones: ids.into_iter().chain(unlisted).map(named).collect(),
                listed,
            }
        })
    }

    /// QTimeZonePrivate::findLongNamePrefix without a time: the first zone, in
    /// getCanonicalZonesThenAliases's order, that `text` starts with a long name of in the
    /// C locale, whatever the case of its letters, and the name's length in characters. The
    /// name is the zone's standard one, or its daylight-saving one if the zone ever has
    /// that time; a later zone with a longer name wins. When nothing matches, CLDR's
    /// aliases that are not in that order are tried the same way.
    pub fn by_long_name(&self, text: &[char]) -> Option<(Arc<Zone>, usize)> {
        let lower: Vec<char> = text.iter().flat_map(|c| c.to_lowercase()).collect();
        type Best = Option<(Arc<Zone>, usize)>;
        // Whether the zone's name is the best so far and all of the text.
        let try_zone = |best: &mut Best, zone: &(String, LongNames)| {
            let (id, names) = zone;
            let longest = best.as_ref().map_or(0, |b| b.1);
            let fits = |name: &Option<(Vec<char>, usize)>| {
                let (name, len) = name.as_ref()?;
                (*len > longest && lower.starts_with(name)).then_some(*len)
            };
            let (standard, daylight) = (fits(&names[0]), fits(&names[1]));
            if standard.is_none() && daylight.is_none() {
                return false;
            }
            let Some(zone) = self.zone(id) else {
                return false;
            };
            let daylight = daylight.filter(|_| zone.has_daylight_time());
            let Some(len) = standard.max(daylight) else {
                return false;
            };
            *best = Some((zone, len));
            len >= text.len()
        };
        let names = self.names();
        let (listed, unlisted) = names.zones.split_at(names.listed);
        let mut best = None;
        let _ = listed.iter().any(|zone| try_zone(&mut best, zone));
        if best.is_none() {
            let _ = unlisted.iter().any(|zone| try_zone(&mut best, zone));
        }
        best
    }
}

/// The TZif files under `dir`, by their paths from it (`under` is the path so far), as
/// loadTzTimeZones lists them: hidden files, posix/, right/ and links to directories are
/// passed over.
fn list_tzif(dir: &Path, under: &str, ids: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') || (under.is_empty() && (name == "posix" || name == "right")) {
            continue;
        }
        let id = format!("{under}{name}");
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            list_tzif(&entry.path(), &format!("{id}/"), ids);
            continue;
        }
        let mut magic = [0; 4];
        let is_tzif = std::fs::File::open(entry.path())
            .and_then(|mut f| f.read_exact(&mut magic))
            .is_ok_and(|()| &magic == b"TZif");
        if is_tzif {
            ids.insert(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seconds since the epoch of a UTC date and time.
    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        let date = NaiveDate::from_ymd_opt(y, m, d).unwrap();
        date.and_hms_opt(h, min, 0).unwrap().and_utc().timestamp()
    }

    fn posix(rule: &str) -> Option<Zone> {
        let text = rule.as_bytes();
        Rule::parse(text, true).map(|rule| Zone::of_rule(rule, text))
    }

    /// A TZif file of `version` whose 64-bit block has one change, at `when`, from
    /// LMT (+2:30:17) to MSK (+3:00), and whose footer is `rule`.
    fn tzif(version: u8, when: i64, rule: &str) -> Vec<u8> {
        let header = |time: u32| {
            let counts = [0, 0, 0, time, 2, 8u32];
            let mut h = [b"TZif".as_slice(), &[version], &[0; 15]].concat();
            h.extend(counts.iter().flat_map(|c| c.to_be_bytes()));
            h
        };
        let (lmt, msk) = (9017i32.to_be_bytes(), 10800i32.to_be_bytes());
        let kinds = [lmt.as_slice(), &[0, 0], &msk, &[0, 4]].concat();
        let tail = [kinds, b"LMT\0MSK\0".to_vec()].concat();
        let mut file = [header(0), tail.clone()].concat();
        file.extend(header(1));
        file.extend(when.to_be_bytes());
        file.push(1);
        file.extend(tail);
        file.extend(format!("\n{rule}\n").bytes());
        file
    }

    #[test]
    fn a_posix_rule_is_read_as_qt_reads_it() {
        let ahead = |rule: &str, secs| posix(rule).map(|zone| zone.ahead_at(secs));
        let (january, july) = (at(2024, 1, 10, 12, 0), at(2024, 7, 10, 12, 0));
        assert_eq!(ahead("XYZ+5", january), Some(-18000));
        assert_eq!(ahead("XYZ-5:30", january), Some(19800));
        assert_eq!(ahead("XYZ+99", january), Some(-356400));
        assert_eq!(ahead("<+03>-3", january), Some(10800));
        assert_eq!(ahead("GMT0", january), Some(0));
        assert_eq!(ahead("EST5EDT,M3.2.0,M11.1.0", january), Some(-18000));
        assert_eq!(ahead("EST5EDT,M3.2.0,M11.1.0", july), Some(-14400));
        // Southern summer: daylight-saving time holds across the new year.
        assert_eq!(ahead("AEST-10AEDT,M10.1.0,M4.1.0/3", january), Some(39600));
        assert_eq!(ahead("AEST-10AEDT,M10.1.0,M4.1.0/3", july), Some(36000));
        for none in [
            "EST5EDT",
            "XY5",
            "XYZ",
            "XYZ+",
            "XYZ+137",
            "UTC5",
            "GMT+3",
            "XYZ5,M3.2.0",
        ] {
            assert_eq!(ahead(none, january), None, "{none}");
        }
    }

    #[test]
    fn a_time_the_clocks_skip_or_repeat_is_read_from_before_the_change() {
        let berlin = posix("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        assert_eq!(
            berlin.instant_of(at(2024, 3, 31, 1, 59)),
            at(2024, 3, 31, 0, 59)
        );
        assert_eq!(
            berlin.instant_of(at(2024, 3, 31, 2, 30)),
            at(2024, 3, 31, 1, 30)
        );
        assert_eq!(
            berlin.instant_of(at(2024, 3, 31, 3, 0)),
            at(2024, 3, 31, 1, 0)
        );
        assert_eq!(
            berlin.instant_of(at(2024, 10, 27, 2, 30)),
            at(2024, 10, 27, 0, 30)
        );
        assert_eq!(
            berlin.instant_of(at(2024, 10, 27, 3, 0)),
            at(2024, 10, 27, 2, 0)
        );
    }

    #[test]
    fn an_offset_id_is_utc_and_hours_with_minutes_and_seconds() {
        assert_eq!(utc_offset("UTC+03:00:30"), Some(10830));
        assert_eq!(utc_offset("UTC-3"), Some(-10800));
        assert_eq!(utc_offset("UTC+23:59:59"), Some(86399));
        for none in [
            "UTC",
            "UTC+",
            "UTC+24",
            "UTC+3:5",
            "UTC+0300",
            "UTC+03:60",
            "UTC+1:02:03:04",
        ] {
            assert_eq!(utc_offset(none), None, "{none}");
        }
    }

    #[test]
    fn an_offset_zone_is_named_by_its_offset() {
        assert_eq!(offset_id(1), "UTC+00:00:01");
        assert_eq!(offset_id(-12600), "UTC-03:30");
        assert_eq!(offset_id(10830), "UTC+03:00:30");
    }

    #[test]
    fn a_zone_names_its_time_from_its_file_then_from_its_rule() {
        let change = at(1930, 6, 21, 0, 0);
        let (winter, summer) = (at(2024, 1, 10, 12, 0), at(2024, 7, 10, 12, 0));
        let zone = Zone::from_tzif(&tzif(b'2', change, "EST5EDT,M3.2.0,M11.1.0")).unwrap();
        assert_eq!(zone.name_at(change - 1), ("LMT", false));
        assert_eq!(zone.name_at(change), ("MSK", false));
        assert_eq!(zone.name_at(winter), ("EST", false));
        assert_eq!(zone.name_at(summer), ("EDT", true));
        // Without a rule the last change's name holds on.
        let zone = Zone::from_tzif(&tzif(b'3', change, "")).unwrap();
        assert_eq!(zone.name_at(summer), ("MSK", false));
        // A name in angle brackets; a second time at the first's offset is not daylight.
        let zone = posix("<+03>-3").unwrap();
        assert_eq!(zone.name_at(summer), ("+03", false));
        let zone = posix("ABC-3DEF-3,M3.2.0,M11.1.0").unwrap();
        assert_eq!(zone.name_at(winter), ("ABC", false));
        assert_eq!(zone.name_at(summer), ("DEF", false));
        assert_eq!(Zone::fixed(3600).name_at(summer), ("", false));
    }

    #[test]
    fn a_tzif_file_gives_its_changes_then_its_rule() {
        let change = at(1930, 6, 21, 0, 0);
        let zone = Zone::from_tzif(&tzif(b'2', change, "EST5EDT,M3.2.0,M11.1.0")).unwrap();
        assert_eq!(zone.ahead_at(change - 1), 9017);
        assert_eq!(zone.ahead_at(change), 10800);
        assert_eq!(zone.ahead_at(at(2024, 7, 10, 12, 0)), -14400);
        // The name before the first change, the one in 1900, the last standard one.
        assert_eq!(zone.local_names(), ["LMT", "LMT", "MSK"]);
        // Without a rule the last change holds on.
        let zone = Zone::from_tzif(&tzif(b'3', change, "")).unwrap();
        assert_eq!(zone.ahead_at(at(2024, 7, 10, 12, 0)), 10800);
        let whole = tzif(b'2', change, "XYZ5");
        // Cut short, a version Qt 6.12 does not read, a rule Qt does not take.
        assert_eq!(Zone::from_tzif(&whole[..whole.len() - 1]), None);
        assert_eq!(Zone::from_tzif(&whole[..60]), None);
        assert_eq!(Zone::from_tzif(&tzif(b'4', change, "XYZ5")), None);
        assert_eq!(Zone::from_tzif(&tzif(b'2', change, "EST5EDT")), None);
    }

    #[test]
    fn the_database_lists_zone_tab_and_the_tzif_files_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = tzif(b'2', 0, "XYZ5");
        for name in ["My/Zone", "posix/Zone", "Listed"] {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &file).unwrap();
        }
        std::fs::write(dir.path().join("My/Text"), "not a zone").unwrap();
        let table = "# comment\nXX\t+0000+00000\tListed\nXX\t+0000+00000\tGone\tcomment\n";
        std::fs::write(dir.path().join("zone.tab"), table).unwrap();
        let database = Database::in_dirs(vec![dir.path().into()]);
        let ids: Vec<&str> = database.ids.iter().map(String::as_str).collect();
        assert_eq!(ids, ["Gone", "Listed", "My/Zone"]);
        assert_eq!(
            database.zone("My/Zone").map(|z| z.ahead_at(1)),
            Some(-18000)
        );
        // A listed id without a file is taken as a rule, which is UTC without an offset.
        assert_eq!(database.zone("Gone").map(|z| z.ahead_at(1)), Some(0));
        // A file that is not listed, one that is no TZif file, and no such id.
        for none in ["posix/Zone", "My/Text", "Europe/Berlin"] {
            assert_eq!(database.zone(none), None, "{none}");
        }
        // Rules and offsets need no file.
        assert_eq!(database.zone("XYZ+5").map(|z| z.ahead_at(1)), Some(-18000));
        assert_eq!(
            database.zone("UTC+03:00:30").map(|z| z.ahead_at(1)),
            Some(10830)
        );
        assert_eq!(database.zone("UTC").map(|z| z.ahead_at(1)), Some(0));
        // An offset's id is the table's own or the offset written out; a zone has none.
        assert_eq!(database.utc_id("UTC").as_deref(), Some("UTC"));
        assert_eq!(database.utc_id("UTC+3").as_deref(), Some("UTC+03:00"));
        assert_eq!(
            database.utc_id("UTC+03:00:30").as_deref(),
            Some("UTC+03:00:30")
        );
        assert_eq!(database.utc_id("My/Zone"), None);
        assert_eq!(database.utc_id("XYZ+5"), None);

        // Without zone.tab no file is a zone.
        std::fs::remove_file(dir.path().join("zone.tab")).unwrap();
        let database = Database::in_dirs(vec![dir.path().into()]);
        assert_eq!(database.zone("My/Zone"), None);
        assert_eq!(database.zone("XYZ+5").map(|z| z.ahead_at(1)), Some(-18000));
        let name: Vec<char> = "coordinated universal time".chars().collect();
        assert_eq!(
            database.by_long_name(&name).map(|z| (z.0.ahead_at(1), z.1)),
            Some((0, 26))
        );
        let name: Vec<char> = "Eastern Standard Time".chars().collect();
        assert_eq!(database.by_long_name(&name), None);
    }

    /// A TZif file of a zone always `ahead` seconds, which are whole hours, ahead of UTC.
    fn steady(ahead: i32) -> Vec<u8> {
        let mut block = [b"TZif2".as_slice(), &[0; 15]].concat();
        block.extend([0, 0, 0, 0, 1, 4u32].iter().flat_map(|c| c.to_be_bytes()));
        block.extend(ahead.to_be_bytes());
        block.extend(b"\0\0XYZ\0");
        let mut file = block.repeat(2);
        file.extend(format!("\nXYZ{}\n", -ahead / 3600).bytes());
        file
    }

    /// A database in `dir` with zones `Two/Ahead` and `Five/Ahead`, 2 and 5 hours ahead of
    /// UTC, whose /etc/localtime is `dir`/localtime.
    fn database_with_localtime(dir: &Path) -> Database {
        for (name, ahead) in [("Two/Ahead", 7200), ("Five/Ahead", 18000)] {
            std::fs::create_dir_all(dir.join(name).parent().unwrap()).unwrap();
            std::fs::write(dir.join(name), steady(ahead)).unwrap();
        }
        let mut database = Database::in_dirs(vec![dir.into()]);
        database.localtime = dir.join("localtime");
        database
    }

    #[test]
    fn the_system_zone_follows_etc_localtime() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let database = database_with_localtime(dir.path());
        let localtime = dir.path().join("localtime");
        let ahead = || database.local_by(None).map(|zone| zone.ahead_at(1));
        // No file: UTC.
        assert_eq!(ahead(), None);
        symlink(dir.path().join("Two/Ahead"), &localtime).unwrap();
        assert_eq!(ahead(), Some(7200));
        // The link names another zone, as after `timedatectl set-timezone`.
        std::fs::remove_file(&localtime).unwrap();
        symlink(dir.path().join("Five/Ahead"), &localtime).unwrap();
        assert_eq!(ahead(), Some(18000));
        // Another file took its place.
        let new = dir.path().join("new");
        std::fs::write(&new, steady(3600)).unwrap();
        std::fs::rename(&new, &localtime).unwrap();
        assert_eq!(ahead(), Some(3600));
        // The same file was written to.
        let earlier = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        let file = std::fs::File::options().write(true).open(&localtime);
        let mut file = file.unwrap();
        std::io::Write::write_all(&mut file, &steady(-3600)).unwrap();
        file.set_modified(earlier).unwrap();
        drop(file);
        assert_eq!(ahead(), Some(-3600));
        std::fs::remove_file(&localtime).unwrap();
        assert_eq!(ahead(), None);
    }

    #[test]
    fn the_system_zone_follows_tz_as_the_c_library_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let database = database_with_localtime(dir.path());
        std::fs::write(dir.path().join("localtime"), steady(3600)).unwrap();
        let winter = at(2024, 1, 10, 12, 0);
        let summer = at(2024, 7, 10, 12, 0);
        let ahead = |tz: Option<&str>| {
            let zone = database.local_by(tz);
            zone.map(|zone| [winter, summer].map(|at| zone.ahead_at(at)))
        };
        let five = dir.path().join("Five/Ahead");
        // What Qt 6.12's Date and QDateTime read with each `TZ` (tools/testenv image).
        let cases = [
            (None, Some([3600, 3600])),
            (Some("Two/Ahead"), Some([7200, 7200])),
            (Some(":Five/Ahead"), Some([18000, 18000])),
            (None, Some([3600, 3600])),
            (Some(five.to_str().unwrap()), Some([18000, 18000])),
            // The empty value is UTC, not /etc/localtime.
            (Some(""), Some([0, 0])),
            // No such file, and no rule: UTC.
            (Some("No/Such"), None),
            // A file of the other directories of the database is not looked for.
            (Some("Europe/Moscow"), None),
            // POSIX rules: the offset is what UTC is ahead by.
            (Some("XYZ-3"), Some([10800, 10800])),
            (Some("UTC+3"), Some([-10800, -10800])),
            (Some("<+0530>-5:30"), Some([19800, 19800])),
            (Some("CET-1CEST,M3.5.0,M10.5.0/3"), Some([3600, 7200])),
            // Daylight-saving time without dates is the United States'.
            (Some("XYZ5ABC"), Some([-18000, -14400])),
            // A rule needs an offset.
            (Some("XYZ"), None),
        ];
        for (tz, zone) in cases {
            assert_eq!(ahead(tz), zone, "TZ={tz:?}");
        }
    }

    #[test]
    fn partition_swaps_as_libstdcxx_does() {
        let mut all = [1, 8, 3, 6, 5, 4, 7, 2];
        partition(&mut all, |n| n % 2 == 1);
        assert_eq!(all, [1, 7, 3, 5, 6, 4, 8, 2]);
    }
}
