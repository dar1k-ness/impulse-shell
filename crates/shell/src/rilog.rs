//! Quickshell's logging, natively: the `log.qslog` encoding (core/logging.cpp
//! `EncodedLogWriter`/`EncodedLogReader`, log version 2), `LogMessage::formatMessage`,
//! `QT_LOGGING_RULES`-style rules (logging_qtprivate.cpp), `qs log` reading/following, and the
//! instance log writer (`ThreadLogging`) that fills `log.qslog` and `log.log` for this process.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::{OnceLock, mpsc};

const LOG_VERSION: u8 = 2;
const OP_REGISTER_CATEGORY: u32 = 0;
const OP_RECENT_SHORT: u32 = 1;
const OP_RECENT_LONG: u32 = 2;
const BEGIN_CATEGORIES: u32 = 3;
const RECENT: usize = 256;

pub const BARE: &[u8] = b"quickshell.bare";
pub const LOGGING: &[u8] = b"quickshell.logging";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Critical,
    Fatal,
}

impl Level {
    fn compressed(self) -> u8 {
        match self {
            Level::Debug => 0,
            Level::Info => 1,
            Level::Warn => 2,
            Level::Critical | Level::Fatal => 3,
        }
    }

    fn of_compressed(v: u8) -> Self {
        match v {
            0 => Level::Debug,
            2 => Level::Warn,
            3 => Level::Critical,
            _ => Level::Info,
        }
    }
}

/// A message without its time; equality is `LogMessage::operator==`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Msg {
    pub ty: Level,
    pub category: Vec<u8>,
    pub body: Vec<u8>,
}

/// `CategoryFilter`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filter {
    pub debug: bool,
    pub info: bool,
    pub warn: bool,
    pub critical: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            debug: true,
            info: true,
            warn: true,
            critical: true,
        }
    }
}

impl Filter {
    pub fn shows(&self, ty: Level) -> bool {
        match ty {
            Level::Debug => self.debug,
            Level::Info => self.info,
            Level::Warn => self.warn,
            Level::Critical => self.critical,
            Level::Fatal => true,
        }
    }

    pub fn apply(&mut self, category: &[u8], rule: &Rule) {
        for (ty, slot) in [
            (Level::Debug, &mut self.debug),
            (Level::Info, &mut self.info),
            (Level::Warn, &mut self.warn),
            (Level::Critical, &mut self.critical),
        ] {
            let pass = rule.pass(category, ty);
            if pass != 0 {
                *slot = pass > 0;
            }
        }
    }

    fn flags(&self) -> u8 {
        self.debug as u8
            | (self.info as u8) << 1
            | (self.warn as u8) << 2
            | (self.critical as u8) << 3
    }

    fn from_flags(f: u8) -> Self {
        Filter {
            debug: f & 1 != 0,
            info: f & 2 != 0,
            warn: f & 4 != 0,
            critical: f & 8 != 0,
        }
    }
}

bits::bit_flags! {
const FULL_TEXT: u8 = 1;
const LEFT: u8 = 2;
const RIGHT: u8 = 4;
}
const MID: u8 = LEFT | RIGHT;

/// Qt's `QLoggingRule`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    category: Vec<u16>,
    flags: u8,
    ty: Option<Level>,
    enabled: bool,
}

impl Rule {
    fn new(pattern: &[u16], enabled: bool) -> Self {
        let mut ty = None;
        let mut p = pattern;
        for (suffix, t) in [
            (".debug", Level::Debug),
            (".info", Level::Info),
            (".warning", Level::Warn),
            (".critical", Level::Critical),
        ] {
            let s: Vec<u16> = suffix.encode_utf16().collect();
            if p.ends_with(&s) {
                p = &p[..p.len() - s.len()];
                ty = Some(t);
                break;
            }
        }
        let star = u16::from(b'*');
        let mut flags = 0;
        if !p.contains(&star) {
            flags = FULL_TEXT;
        } else {
            if p.ends_with(&[star]) {
                flags |= LEFT;
                p = &p[..p.len() - 1];
            }
            if p.starts_with(&[star]) {
                flags |= RIGHT;
                p = &p[1..];
            }
            if p.contains(&star) {
                flags = 0;
            }
        }
        Rule {
            category: p.to_vec(),
            flags,
            ty,
            enabled,
        }
    }

    fn pass(&self, cat: &[u8], ty: Level) -> i32 {
        if self.ty.is_some_and(|t| t != ty) {
            return 0;
        }
        let verdict = if self.enabled { 1 } else { -1 };
        let cat: Vec<u16> = cat.iter().map(|&b| u16::from(b)).collect();
        if self.flags == FULL_TEXT {
            return if self.category == cat { verdict } else { 0 };
        }
        let n = self.category.len();
        let Some(idx) = (0..=cat.len().saturating_sub(n))
            .find(|&i| cat.len() >= n && cat[i..i + n] == self.category[..])
        else {
            return 0;
        };
        match self.flags {
            MID => verdict,
            LEFT if idx == 0 => verdict,
            RIGHT if idx == cat.len() - n => verdict,
            _ => 0,
        }
    }
}

/// `QLoggingSettingsParser` over a `;`-separated spec: the rules, and the malformed entries Qt
/// warns about ("Ignoring malformed logging rule: '...'").
pub fn parse_rules(spec: &str) -> (Vec<Rule>, Vec<String>) {
    let (mut rules, mut bad) = (Vec::new(), Vec::new());
    for line in spec.split(';') {
        let line = qt_trim(line);
        let Some(eq) = line.find('=') else { continue };
        if line.rfind('=') != Some(eq) {
            bad.push(line.to_string());
            continue;
        }
        let key: Vec<u16> = qt_trim(&line[..eq]).encode_utf16().collect();
        let value = match qt_trim(&line[eq + 1..]) {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        let rule = Rule::new(&key, value == Some(true));
        if rule.flags != 0 && value.is_some() {
            rules.push(rule);
        } else {
            bad.push(line.to_string());
        }
    }
    (rules, bad)
}

/// `QStringView::trimmed`: strips `QChar::isSpace`.
fn qt_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace())
}

/// Bytes in Latin-1 (`QLatin1StringView`) as UTF-8.
fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

/// `LogMessage::formatMessage`; `time` is the formatted timestamp when timestamps are on.
pub fn format(out: &mut Vec<u8>, msg: &Msg, color: bool, time: Option<&str>, prefix: &str) {
    if !prefix.is_empty() {
        if color {
            out.extend_from_slice(b"\x1b[90m");
        }
        out.push(b'[');
        out.extend_from_slice(prefix.as_bytes());
        out.push(b']');
        if time.is_some() {
            out.push(b' ');
        }
        if color {
            out.extend_from_slice(b"\x1b[0m");
        }
    }
    if let Some(t) = time {
        if color {
            out.extend_from_slice(b"\x1b[90m");
        }
        out.extend_from_slice(t.as_bytes());
    }
    let body = String::from_utf8_lossy(&msg.body);
    if msg.category == BARE {
        if !prefix.is_empty() {
            out.push(b' ');
        }
        out.extend_from_slice(body.as_bytes());
        return;
    }
    let label: &[u8] = match (color, msg.ty) {
        (true, Level::Debug) => b"\x1b[34m DEBUG",
        (true, Level::Info) => b"\x1b[32m  INFO",
        (true, Level::Warn) => b"\x1b[33m  WARN",
        (true, Level::Critical) => b"\x1b[31m ERROR",
        (true, Level::Fatal) => b"\x1b[31m FATAL",
        (false, Level::Debug) => b" DEBUG",
        (false, Level::Info) => b"  INFO",
        (false, Level::Warn) => b"  WARN",
        (false, Level::Critical) => b" ERROR",
        (false, Level::Fatal) => b" FATAL",
    };
    out.extend_from_slice(label);
    let default = msg.category == b"default";
    let fatal = msg.ty == Level::Fatal;
    if color && !default && !fatal {
        out.extend_from_slice(b"\x1b[97m");
    }
    if !default {
        out.push(b' ');
        out.extend_from_slice(latin1(&msg.category).as_bytes());
    }
    if color && !fatal {
        out.extend_from_slice(b"\x1b[0m");
    }
    out.extend_from_slice(b": ");
    out.extend_from_slice(body.as_bytes());
    if color && fatal {
        out.extend_from_slice(b"\x1b[0m");
    }
}

/// Seconds since the epoch as local `yyyy-MM-dd hh:mm:ss.zzz` (`zzz` = `ms`), in the zone
/// `Date` and the clocks read; empty when the time is out of range, like an invalid
/// `QDateTime` (which counts milliseconds in 64 bits).
pub fn local_time(secs: i64, ms: i64) -> String {
    if secs.checked_mul(1000).is_none() {
        return String::new();
    }
    let wall = secs + i64::from(qml::tz::local_ahead(secs));
    let (year, month, day) = crate::ricli::civil(wall.div_euclid(86_400));
    let s = wall.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}.{ms:03}",
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

/// Milliseconds since the epoch now (`QDateTime::currentDateTime`).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

pub fn local_time_ms(ms: i64) -> String {
    local_time(ms.div_euclid(1000), ms.rem_euclid(1000))
}

/// `QDebug` quoting of a `QString`.
pub fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 || (0x7f..0xa0).contains(&(c as u32)) => {
                o.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// `QDebug` quoting of a `QByteArray`.
pub fn quote_bytes(b: &[u8]) -> String {
    let mut o = String::from("\"");
    let mut hex = false;
    for &c in b {
        if hex && c.is_ascii_hexdigit() {
            o.push_str("\"\"");
        }
        hex = false;
        match c {
            b'"' => o.push_str("\\\""),
            b'\\' => o.push_str("\\\\"),
            8 => o.push_str("\\b"),
            12 => o.push_str("\\f"),
            b'\n' => o.push_str("\\n"),
            b'\r' => o.push_str("\\r"),
            b'\t' => o.push_str("\\t"),
            0x20..0x7f => o.push(char::from(c)),
            _ => {
                o.push_str(&format!("\\x{c:02X}"));
                hex = true;
            }
        }
    }
    o.push('"');
    o
}

/// `EncodedLogWriter`.
#[derive(Default)]
pub struct Encoder {
    categories: HashMap<Vec<u8>, u16>,
    next_category: u16,
    recent: VecDeque<Msg>,
    last_secs: i64,
}

fn put_varint(out: &mut Vec<u8>, n: u32) {
    if n < 0xff {
        out.push(n as u8);
    } else if n < 0xffff {
        out.push(0xff);
        out.extend_from_slice(&(n as u16).to_le_bytes());
    } else {
        out.push(0xff);
        out.extend_from_slice(&0xffffu16.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
    }
}

fn put_string(out: &mut Vec<u8>, b: &[u8]) {
    put_varint(out, b.len() as u32);
    out.extend_from_slice(b);
}

impl Encoder {
    pub fn header(out: &mut Vec<u8>) {
        out.push(LOG_VERSION);
    }

    /// The encoder that goes on writing the log `b` (a whole file, header included): the
    /// category ids, recent messages and time a reader of `b` holds at its end. `None` on a
    /// bad header or a malformed tail.
    fn resume(b: &[u8]) -> Option<Self> {
        if b.first() != Some(&LOG_VERSION) {
            return None;
        }
        let mut s = Src { b, pos: 1 };
        let mut d = Decoder::default();
        let mut cursor = s.pos;
        while d.read(&mut s).is_some() {
            cursor = s.pos;
        }
        if cursor != b.len() || s.pos != cursor {
            return None;
        }
        let ids = d.categories.iter().enumerate();
        Some(Self {
            categories: ids.map(|(id, c)| (c.0.clone(), id as u16)).collect(),
            next_category: d.categories.len() as u16,
            recent: d.recent.into_iter().map(|r| r.msg).collect(),
            last_secs: d.last_secs,
        })
    }

    /// Append `msg`, logged at `ms` since the epoch; `filter` gives a new category's flags.
    pub fn write(
        &mut self,
        out: &mut Vec<u8>,
        msg: &Msg,
        ms: i64,
        filter: impl Fn(&[u8]) -> Filter,
    ) {
        let index = self.recent.iter().position(|m| m == msg);
        self.recent.push_front(msg.clone());
        self.recent.truncate(RECENT);
        let delta = (ms - self.last_secs * 1000) / 1000;
        if let Some(index) = index {
            if delta < 16 && index < 16 {
                out.push(OP_RECENT_SHORT as u8);
                out.push((index as i64 | delta << 4) as u8);
            } else {
                out.push(OP_RECENT_LONG as u8);
                out.push(index as u8);
                put_varint(out, delta as u32);
            }
        } else {
            let id = match self.categories.get(&msg.category) {
                Some(&id) => id,
                None => {
                    out.push(OP_REGISTER_CATEGORY as u8);
                    put_string(out, &msg.category);
                    let id = self.next_category;
                    self.next_category = id.wrapping_add(1);
                    self.categories.insert(msg.category.clone(), id);
                    out.push(filter(&msg.category).flags());
                    id
                }
            };
            put_varint(out, u32::from(id) + BEGIN_CATEGORIES);
            let full = (ms / 1000) as u64;
            if msg.ty == Level::Fatal {
                out.push(0xff);
                out.extend_from_slice(&full.to_le_bytes());
            } else {
                let mut field = msg.ty.compressed();
                if delta >= 0x1d {
                    field |= (if delta < 0xffff { 0x1d } else { 0x1e }) << 3;
                } else {
                    field = (i64::from(field) | delta << 3) as u8;
                }
                out.push(field);
                if delta >= 0x1d {
                    if delta > 0xffff {
                        out.extend_from_slice(&full.to_le_bytes());
                    } else {
                        put_varint(out, delta as u32);
                    }
                }
            }
            put_string(out, &msg.body);
        }
        self.last_secs = ms / 1000;
    }
}

/// `QFile` read semantics over the bytes not yet consumed: short reads still advance.
struct Src<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Src<'a> {
    fn read(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = (self.pos + n).min(self.b.len());
        let s = &self.b[self.pos..end];
        self.pos = end;
        (s.len() == n).then_some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        self.read(1).map(|s| s[0])
    }

    fn varint(&mut self) -> Option<u32> {
        let mut bytes = [0u8; 7];
        let rest = &self.b[self.pos..];
        let len = rest.len().min(7);
        bytes[..len].copy_from_slice(&rest[..len]);
        if bytes[0] != 0xff && len >= 1 {
            self.read(1)?;
            Some(u32::from(bytes[0]))
        } else if (bytes[1] != 0xff || bytes[2] != 0xff) && len >= 3 {
            self.read(3)?;
            Some(u32::from(u16::from_le_bytes([bytes[1], bytes[2]])))
        } else if len == 7 {
            self.read(7)?;
            Some(u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]))
        } else {
            None
        }
    }

    fn string(&mut self) -> Option<Vec<u8>> {
        let n = self.varint()? as usize;
        self.read(n).map(<[u8]>::to_vec)
    }
}

/// A decoded message: time in seconds since the epoch and the id its filter is cached under.
#[derive(Clone, Debug)]
pub struct Record {
    pub msg: Msg,
    pub secs: i64,
    category_id: u16,
}

/// `EncodedLogReader`.
#[derive(Default)]
pub struct Decoder {
    categories: Vec<(Vec<u8>, Filter)>,
    recent: VecDeque<Record>,
    last_secs: i64,
}

impl Decoder {
    fn read(&mut self, s: &mut Src) -> Option<Record> {
        let rec = loop {
            let next = s.varint()?;
            if next == OP_REGISTER_CATEGORY {
                let name = s.string()?;
                let flags = s.u8()?;
                self.categories.push((name, Filter::from_flags(flags)));
                continue;
            }
            if next == OP_RECENT_SHORT || next == OP_RECENT_LONG {
                let (index, delta) = if next == OP_RECENT_SHORT {
                    let f = s.u8()?;
                    (f & 0xf, u32::from(f >> 4))
                } else {
                    (s.u8()?, s.varint()?)
                };
                let mut rec = self.recent.get(usize::from(index))?.clone();
                self.last_secs = self.last_secs.wrapping_add(i64::from(delta));
                rec.secs = self.last_secs;
                break rec;
            }
            let id = next - BEGIN_CATEGORIES;
            let category = self
                .categories
                .get(id as usize)
                .map(|c| c.0.clone())
                .unwrap_or_default();
            let field = s.u8()?;
            let (ty, mut delta, mut timed) = if field == 0xff {
                (Level::Fatal, 0u64, true)
            } else {
                (
                    Level::of_compressed(field & 7),
                    u64::from(field >> 3),
                    false,
                )
            };
            if field != 0xff {
                if delta == 0x1d {
                    delta = u64::from(s.varint()?);
                } else if delta == 0x1e {
                    timed = true;
                }
            }
            if timed {
                delta = u64::from_le_bytes(s.read(8)?.try_into().ok()?);
            }
            self.last_secs = self.last_secs.wrapping_add(delta as i64);
            let body = s.string()?;
            break Record {
                msg: Msg { ty, category, body },
                secs: self.last_secs,
                category_id: id as u16,
            };
        };
        self.recent.push_front(rec.clone());
        self.recent.truncate(RECENT);
        Some(rec)
    }

    fn filter(&self, id: u16) -> Filter {
        self.categories
            .get(usize::from(id))
            .map(|c| c.1)
            .unwrap_or_default()
    }
}

/// Decode a whole log (header included) into its records; `None` on a bad header or a
/// malformed tail.
pub fn decode_all(b: &[u8]) -> Option<Vec<Record>> {
    if b.first() != Some(&LOG_VERSION) {
        return None;
    }
    let mut s = Src { b, pos: 1 };
    let mut d = Decoder::default();
    let mut out = Vec::new();
    let mut cursor = s.pos;
    while let Some(r) = d.read(&mut s) {
        cursor = s.pos;
        out.push(r);
    }
    (cursor == b.len() && s.pos == cursor).then_some(out)
}

/// Writes `b` to stdout and flushes it.
fn put(b: &[u8]) -> std::io::Result<()> {
    let mut o = std::io::stdout().lock();
    o.write_all(b)?;
    o.flush()
}

/// Output settings shared by the `qs log` / `qs list` messages (`LogManager`).
pub struct Out {
    pub color: bool,
    pub timestamps: bool,
    pub prefix: &'static str,
}

impl Out {
    /// A message through the log manager (stdout, flushed per line).
    pub fn emit(&self, ty: Level, category: &[u8], body: &str) {
        let msg = Msg {
            ty,
            category: category.to_vec(),
            body: body.as_bytes().to_vec(),
        };
        let time = self.timestamps.then(|| local_time_ms(now_ms()));
        let mut b = Vec::new();
        format(&mut b, &msg, self.color, time.as_deref(), self.prefix);
        b.push(b'\n');
        let _ = put(&b);
    }

    pub fn bare(&self, body: &str) {
        self.emit(Level::Info, BARE, body);
    }
}

/// `LogReader`: the open file, the bytes read but not yet decoded, and the readout state.
struct Reader<'a> {
    file: std::fs::File,
    pending: Vec<u8>,
    dec: Decoder,
    rules: Vec<Rule>,
    filters: HashMap<u16, Filter>,
    tail: usize,
    out: &'a Out,
}

impl Reader<'_> {
    fn initialize(&mut self) -> bool {
        let _ = self.file.read_to_end(&mut self.pending);
        let Some(&version) = self.pending.first() else {
            self.out
                .emit(Level::Critical, b"default", "Failed to read log header.");
            return false;
        };
        self.pending.remove(0);
        if version != LOG_VERSION {
            self.out.emit(
                Level::Critical,
                b"default",
                &format!(
                    "This log was encoded with version {version} of the quickshell log encoder, \
                     which cannot be decoded by the current version of quickshell, with log \
                     version {LOG_VERSION}"
                ),
            );
            return false;
        }
        true
    }

    fn continue_reading(&mut self) -> bool {
        let _ = self.file.read_to_end(&mut self.pending);
        let data = std::mem::take(&mut self.pending);
        let mut s = Src { b: &data, pos: 0 };
        let mut cursor = 0;
        let mut text = Vec::new();
        let mut ring: VecDeque<Record> = VecDeque::new();
        while let Some(rec) = self.dec.read(&mut s) {
            cursor = s.pos;
            let filter = *self.filters.entry(rec.category_id).or_insert_with(|| {
                let mut f = self.dec.filter(rec.category_id);
                for r in &self.rules {
                    f.apply(&rec.msg.category, r);
                }
                f
            });
            if !filter.shows(rec.msg.ty) {
                continue;
            }
            if self.tail == 0 {
                self.line(&mut text, &rec);
            } else {
                ring.push_front(rec);
                ring.truncate(self.tail);
            }
        }
        for rec in ring.iter().rev() {
            self.line(&mut text, rec);
        }
        if let Err(e) = put(&text) {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                crate::shutdown::exit_by(libc::SIGPIPE);
            }
            // An output that cannot be written (a full disk under a redirection): the
            // command ends as well instead of following a log nobody gets.
            return false;
        }
        if s.pos == cursor {
            // An incomplete varint is left for the next read, as Qt's peek leaves it unread.
            self.pending = data[cursor..].to_vec();
        } else {
            self.out.emit(
                Level::Critical,
                b"default",
                "An error occurred parsing the end of this log file.",
            );
            self.out.emit(
                Level::Critical,
                b"default",
                &format!("Remaining data: {}", quote_bytes(&data[s.pos..])),
            );
            return false;
        }
        true
    }

    /// Follows the file now at `path` if it is not the one being read (a rotation put a new
    /// one there): the rest of this one first, then the new one from its header. `Some(false)`:
    /// the same file, or none at `path`; `None`: a read failed.
    fn reopen(&mut self, path: &Path) -> Option<bool> {
        use std::os::unix::fs::MetadataExt;
        let id = |f: &std::fs::File| f.metadata().ok().map(|m| (m.dev(), m.ino()));
        let Ok(next) = std::fs::File::open(path) else {
            return Some(false);
        };
        if id(&next).is_none() || id(&next) == id(&self.file) {
            return Some(false);
        }
        if !self.continue_reading() {
            return None;
        }
        self.file = next;
        self.pending.clear();
        // Category ids and the time base are per file.
        self.dec = Decoder::default();
        self.filters.clear();
        (self.initialize() && self.continue_reading()).then_some(true)
    }

    fn line(&self, text: &mut Vec<u8>, rec: &Record) {
        let time = self.out.timestamps.then(|| local_time(rec.secs, 0));
        format(text, &rec.msg, self.out.color, time.as_deref(), "");
        text.push(b'\n');
    }
}

enum FollowEvent {
    /// The file followed was written to.
    Changed,
    /// A file was put at the path (a rotation's successor).
    Replaced,
    /// No writer locks the file opened as this generation any more.
    Unlocked(u64),
}

type FollowTx = mpsc::Sender<(FollowEvent, Option<std::io::Error>)>;

/// `qs log`'s `readEncodedLogs`: print the log at `path` (already opened), then with `follow`
/// keep printing what is appended until the writer's lock goes away. A rotation does not end
/// it: the writer puts the next file (locked, with its header) at `path` before it lets go of
/// the old one, which is read to its end before the new one. Returns success. A write to an
/// output nobody reads any more (`rusty-impulse log -f | head -1`) ends the process by SIGPIPE, as it
/// ends `qs log`, instead of following the log, unread, until its writer exits.
pub fn read_logs(
    file: std::fs::File,
    path: &Path,
    out: &Out,
    tail: usize,
    follow: bool,
    rulespec: &str,
) -> bool {
    let (rules, bad) = parse_rules(rulespec);
    for line in bad {
        out.emit(
            Level::Warn,
            LOGGING,
            &format!("Ignoring malformed logging rule: '{line}'"),
        );
    }
    let mut r = Reader {
        file,
        pending: Vec::new(),
        dec: Decoder::default(),
        rules,
        filters: HashMap::new(),
        tail,
        out,
    };
    if !r.initialize() || !r.continue_reading() {
        return false;
    }
    if !follow {
        return true;
    }
    let (tx, rx) = mpsc::channel();
    if let Err(e) = watch_file(path, tx.clone()) {
        // Without the watch nothing appended is shown until the writer lets go of the file.
        elog::eprintln!(
            "rusty-impulse log: cannot watch {} for changes: {e}",
            path.display()
        );
    }
    // What was appended between the read above and the watch: no event reports it, and it
    // would wait for the next write to the log.
    if !r.continue_reading() {
        return false;
    }
    let mut generation = 0;
    if !wait_unlocked(&r.file, generation, tx.clone()) {
        return false;
    }
    let follow_next = |r: &mut Reader, generation: &mut u64| {
        *generation += 1;
        wait_unlocked(&r.file, *generation, tx.clone())
    };
    while let Ok((ev, err)) = rx.recv() {
        match ev {
            FollowEvent::Changed => {
                if !r.continue_reading() {
                    return false;
                }
            }
            FollowEvent::Replaced => match r.reopen(path) {
                Some(true) => {
                    if !follow_next(&mut r, &mut generation) {
                        return false;
                    }
                }
                Some(false) => {}
                None => return false,
            },
            // A rotated-away file's writer let go of it: its successor is followed already.
            FollowEvent::Unlocked(g) if g != generation => {}
            FollowEvent::Unlocked(_) => {
                if let Some(e) = err {
                    out.emit(
                        Level::Warn,
                        LOGGING,
                        &format!(
                            "Failed to wait for write locks to be removed from log file with error code {}: {}",
                            e.raw_os_error().unwrap_or(0),
                            qt_error_string(&e)
                        ),
                    );
                    return r.continue_reading();
                }
                // Let go of by a rotation whose successor the watch has not reported yet, or
                // by a writer that ended.
                match r.reopen(path) {
                    Some(true) => {
                        if !follow_next(&mut r, &mut generation) {
                            return false;
                        }
                    }
                    Some(false) => return r.continue_reading(),
                    None => return false,
                }
            }
        }
    }
    false
}

/// Sends `Unlocked(generation)` once no process write-locks `file`: a read lock taken with
/// F_SETLKW (fcntl(2)) on a thread of its own.
fn wait_unlocked(file: &std::fs::File, generation: u64, tx: FollowTx) -> bool {
    // A refused thread drops `lock_file` with the closure: the copy is closed.
    file.try_clone()
        .and_then(|lock_file| spawn_lock_wait(lock_file, generation, tx))
        .inspect_err(|e| {
            elog::eprintln!(
                "rusty-impulse log: cannot wait for the log's writer to let go of it: {e}"
            );
        })
        .is_ok()
}

fn spawn_lock_wait(lock_file: std::fs::File, generation: u64, tx: FollowTx) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("qs-log-lock".into())
        .spawn(move || {
            let fl = libc::flock {
                l_type: libc::F_RDLCK as _,
                l_whence: libc::SEEK_SET as _,
                l_start: 0,
                l_len: 0,
                l_pid: 0,
            };
            // SAFETY: F_SETLKW on the open lock file takes a pointer to an initialized flock.
            let res = unsafe { libc::fcntl(lock_file.as_raw_fd(), libc::F_SETLKW, &fl) };
            let err = (res != 0).then(std::io::Error::last_os_error);
            let _ = tx.send((FollowEvent::Unlocked(generation), err));
        })
        .map(drop)
}

/// `qt_error_string`: strerror with Qt's own wording for a few codes.
fn qt_error_string(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(0) => "No error".into(),
        Some(libc::EACCES) => "Permission denied".into(),
        Some(libc::EMFILE) => "Too many open files".into(),
        Some(libc::ENOENT) => "No such file or directory".into(),
        Some(libc::ENOSPC) => "No space left on device".into(),
        Some(n) => {
            let mut buf = [0u8; 256];
            // SAFETY: `buf` is writable for the length passed; the XSI strerror_r (what libc
            // binds on Linux) writes a NUL-terminated message ("Unknown error N" for an unknown
            // `n`, as strerror) and keeps no pointer to it.
            unsafe { libc::strerror_r(n, buf.as_mut_ptr().cast(), buf.len()) };
            std::ffi::CStr::from_bytes_until_nul(&buf)
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|_| e.to_string())
        }
        None => e.to_string(),
    }
}

/// `QFileSystemWatcher::fileChanged` for the file at `path`, on its own inotify thread, and
/// `Replaced` when a file is created or renamed to `path` (then watched too). An error is
/// inotify's or the refused thread's; the inotify descriptor is closed then.
fn watch_file(path: &Path, tx: FollowTx) -> std::io::Result<()> {
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    let bad_path = || std::io::Error::from(std::io::ErrorKind::InvalidInput);
    const FILE: u32 = libc::IN_MODIFY | libc::IN_ATTRIB | libc::IN_MOVE_SELF | libc::IN_DELETE_SELF;
    let (Some(name), Ok(cpath)) = (
        path.file_name().map(|n| n.as_bytes().to_vec()),
        std::ffi::CString::new(path.as_os_str().as_bytes()),
    ) else {
        return Err(bad_path());
    };
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let cdir = std::ffi::CString::new(dir.as_os_str().as_bytes()).map_err(|_| bad_path())?;
    // SAFETY: inotify_init1 takes only flags.
    let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: inotify_init1 just returned this descriptor, owned by nothing else. Closed when
    // dropped: on an error below, with the closure of a refused thread, or as the thread ends.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: `fd` is the inotify fd just created and `cpath` a NUL-terminated path.
    let file_wd = unsafe { libc::inotify_add_watch(fd, cpath.as_ptr(), FILE) };
    if file_wd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let dir_mask = libc::IN_CREATE | libc::IN_MOVED_TO;
    // SAFETY: as above, with `cdir`.
    let dir_wd = unsafe { libc::inotify_add_watch(fd, cdir.as_ptr(), dir_mask) };
    std::thread::Builder::new()
        .name("qs-log-watch".into())
        .spawn(move || {
            let fd = owned.as_raw_fd();
            let mut buf = [0u8; 4096];
            loop {
                // SAFETY: `buf` is writable for the length passed; `owned` keeps `fd` open
                // until this thread ends.
                let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
                if n <= 0 {
                    break;
                }
                let (mut changed, mut replaced) = (false, false);
                for (wd, ev_name) in inotify_events(&buf[..n as usize]) {
                    if wd != dir_wd {
                        changed = true;
                    } else if ev_name == name.as_slice() {
                        replaced = true;
                    }
                }
                if replaced {
                    // Watched before it is reported, so no write to it goes unseen.
                    // SAFETY: as above.
                    unsafe { libc::inotify_add_watch(fd, cpath.as_ptr(), FILE) };
                }
                let sent = (!changed || tx.send((FollowEvent::Changed, None)).is_ok())
                    && (!replaced || tx.send((FollowEvent::Replaced, None)).is_ok());
                if !sent {
                    break;
                }
            }
        })
        .map(drop)
}

/// The watch descriptor and name (NUL padding stripped) of each `struct inotify_event` in
/// what one read(2) of an inotify fd returned (inotify(7)).
fn inotify_events(mut buf: &[u8]) -> impl Iterator<Item = (i32, &[u8])> {
    const HEADER: usize = std::mem::size_of::<libc::inotify_event>();
    std::iter::from_fn(move || {
        let head = buf.get(..HEADER)?;
        let wd = i32::from_ne_bytes(head[0..4].try_into().ok()?);
        let len = u32::from_ne_bytes(head[12..16].try_into().ok()?) as usize;
        let name = buf.get(HEADER..HEADER + len)?;
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(len)];
        buf = &buf[HEADER + len..];
        Some((wd, name))
    })
}

struct Entry {
    msg: Msg,
    ms: i64,
}

/// What the `qs-log` thread receives: a message, or a request to close the files (the thread
/// answers once everything before it is written).
enum Job {
    Entry(Entry),
    /// Write out everything queued before it, answer, and go on logging.
    Flush(mpsc::Sender<()>),
    Close(mpsc::Sender<()>),
    /// Flush, and set whether the detailed log's descriptor stays open across an exec; the
    /// answer is that descriptor.
    KeepOpen(bool, mpsc::Sender<Option<i32>>),
}

/// `mpsc::Sender` is `Sync` (since Rust 1.72): every thread logs through it without a lock.
static SINK: OnceLock<mpsc::Sender<Job>> = OnceLock::new();

/// Log a message for this instance (`qInfo()` etc.): kept in `log.qslog`/`log.log` once
/// instance logging started, else dropped like Qt's disabled categories.
pub fn log(ty: Level, category: &str, body: &str) {
    let Some(tx) = SINK.get() else { return };
    let msg = Msg {
        ty,
        category: category.as_bytes().to_vec(),
        body: body.as_bytes().to_vec(),
    };
    let _ = tx.send(Job::Entry(Entry { msg, ms: now_ms() }));
}

/// Writes out everything logged so far and closes the log files; later messages are dropped.
/// Returns once the `qs-log` thread has flushed them, so the process can exit right after.
pub fn close() {
    if let Some(tx) = SINK.get() {
        close_sink(tx);
    }
}

/// Writes out everything logged so far and keeps logging. Returns once the `qs-log` thread has
/// written it, so an `exec` right after (`Rustshell.reload()`) loses nothing, and a failed
/// one leaves the shell logging as before.
pub fn flush() {
    if let Some(tx) = SINK.get() {
        flush_sink(tx);
    }
}

/// The `qs-log` thread, once it runs.
static WRITER: OnceLock<std::thread::ThreadId> = OnceLock::new();

/// [`flush`] for the panic hook: the record of the panic is in the files when this returns, so
/// an exit or an abort right after it loses nothing. The `qs-log` thread itself does not wait
/// when it panics: nobody else reads its queue. Its record reaches stderr only.
pub fn flush_in_panic() {
    if WRITER.get() != Some(&std::thread::current().id()) {
        flush();
    }
}

/// Sends [`Job::Close`] and waits for the answer. If the thread is gone, `send` fails or `recv`
/// returns at once (the dropped `done` disconnects it), so this never hangs on a dead writer.
fn close_sink(tx: &mpsc::Sender<Job>) {
    ask(tx, Job::Close);
}

/// [`close_sink`] for [`Job::Flush`].
fn flush_sink(tx: &mpsc::Sender<Job>) {
    ask(tx, Job::Flush);
}

fn ask(tx: &mpsc::Sender<Job>, job: fn(mpsc::Sender<()>) -> Job) {
    let (done, rx) = mpsc::channel();
    if tx.send(job(done)).is_ok() {
        let _ = rx.recv();
    }
}

/// Names the detailed log's descriptor, open across a reload's exec ([`keep_across_exec`]).
pub const INHERITED_ENV: &str = "RI_LOG_FD";

/// The detailed log of the image that re-executed itself into this one, still open and
/// write-locked: closed on drop.
pub struct Inherited(std::fs::File);

/// The file [`INHERITED_ENV`] names, if it is an open regular file; the caller removes the
/// variable.
pub fn inherited() -> Option<Inherited> {
    use std::os::fd::FromRawFd;
    let fd: i32 = std::env::var(INHERITED_ENV).ok()?.parse().ok()?;
    // SAFETY: `stat` is a C struct of integers, valid all-zero.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fstat(2) of any number only fills `st` or fails with EBADF.
    let r = unsafe { libc::fstat(fd, &mut st) };
    if fd < 0 || r != 0 || st.st_mode & libc::S_IFMT != libc::S_IFREG {
        return None;
    }
    // SAFETY: fcntl(2) on the descriptor fstat just found open.
    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    // SAFETY: an open file the previous image left to this one; nothing else here owns it.
    Some(Inherited(unsafe { std::fs::File::from_raw_fd(fd) }))
}

/// Lets the detailed log stay open across the exec of a reload, after writing what is queued:
/// its write lock is the process's and stays with the descriptor (fcntl(2)), so `qs log -f`
/// goes on following the instance. Returns the variable that names the descriptor to the next
/// image; [`close_on_exec`] undoes it when the exec fails. `None` without a detailed log.
pub fn keep_across_exec() -> Option<(&'static str, String)> {
    keep_open(true).map(|fd| (INHERITED_ENV, fd.to_string()))
}

/// Closes the detailed log on exec again, after [`keep_across_exec`] and a failed exec.
pub fn close_on_exec() {
    keep_open(false);
}

fn keep_open(keep: bool) -> Option<i32> {
    let (done, rx) = mpsc::channel();
    SINK.get()?.send(Job::KeepOpen(keep, done)).ok()?;
    rx.recv().ok()?
}

/// Size at which a log file is rotated to `<name>.1` (replacing the previous one), so a shell
/// that runs for weeks keeps at most twice this per file in the run dir (a tmpfs).
const ROTATE_AT: u64 = 16 << 20;

/// One instance log file, buffered: the `qs-log` thread flushes it whenever its queue is
/// empty, so a burst of messages costs one write(2) and nothing waits in memory while idle.
struct LogFile {
    out: std::io::BufWriter<std::fs::File>,
    path: std::path::PathBuf,
    written: u64,
    /// Rotation size, [`ROTATE_AT`].
    limit: u64,
    /// Detailed (`.qslog`) files are write-locked and start with a header.
    detailed: bool,
}

impl LogFile {
    fn open(path: std::path::PathBuf, detailed: bool) -> std::io::Result<Self> {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        if detailed {
            write_lock(&f, &path);
        }
        let mut lf = Self {
            out: std::io::BufWriter::with_capacity(64 << 10, f),
            path,
            written: 0,
            limit: ROTATE_AT,
            detailed,
        };
        if detailed {
            let mut h = Vec::new();
            Encoder::header(&mut h);
            lf.write(&h)?;
        }
        Ok(lf)
    }

    /// The text log at `path`, which the image before a reload wrote: appended to.
    fn append(path: std::path::PathBuf) -> Option<Self> {
        let f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .ok()?;
        let written = f.metadata().ok()?.len();
        Some(Self {
            out: std::io::BufWriter::with_capacity(64 << 10, f),
            path,
            written,
            limit: ROTATE_AT,
            detailed: false,
        })
    }

    /// The detailed log `file`, which the image before a reload wrote, with the encoder that
    /// goes on in it. `None` unless it is the file at `path` and decodes to its end.
    fn resume(path: std::path::PathBuf, mut file: std::fs::File) -> Option<(Self, Encoder)> {
        use std::io::Seek as _;
        use std::os::unix::fs::MetadataExt as _;
        let (at, own) = (std::fs::metadata(&path).ok()?, file.metadata().ok()?);
        if (at.dev(), at.ino()) != (own.dev(), own.ino()) {
            return None;
        }
        write_lock(&file, &path);
        // Read through this descriptor: closing another one of the file would release the
        // lock (fcntl(2)). The position ends up where the next message goes.
        let mut b = Vec::new();
        file.seek(std::io::SeekFrom::Start(0)).ok()?;
        file.read_to_end(&mut b).ok()?;
        let enc = Encoder::resume(&b)?;
        let lf = Self {
            out: std::io::BufWriter::with_capacity(64 << 10, file),
            path,
            written: b.len() as u64,
            limit: ROTATE_AT,
            detailed: true,
        };
        Some((lf, enc))
    }

    fn write(&mut self, b: &[u8]) -> std::io::Result<()> {
        self.written += b.len() as u64;
        self.out.write_all(b)
    }

    /// Past [`ROTATE_AT`]: keep the file as `<name>.1` and go on in a new one. The new one is
    /// complete (write-locked, header written) before rename(2) puts it at `<name>` in one
    /// step, and the old one stays locked until then, so `qs log -f` finds the successor
    /// whenever it sees the old one's lock go away, and follows it.
    fn rotate_if_full(&mut self) -> std::io::Result<bool> {
        if self.written < self.limit {
            return Ok(false);
        }
        self.out.flush()?;
        let with = |ext: &str| {
            let mut p = self.path.as_os_str().to_owned();
            p.push(ext);
            std::path::PathBuf::from(p)
        };
        let (kept, staged) = (with(".1"), with(".new"));
        let mut next = Self::open(staged.clone(), self.detailed)?;
        let _ = std::fs::remove_file(&kept);
        // A second name for the old file: `<name>` itself is replaced, never missing.
        let _ = std::fs::hard_link(&self.path, &kept);
        if let Err(e) = next
            .out
            .flush()
            .and_then(|()| std::fs::rename(&staged, &self.path))
        {
            let _ = std::fs::remove_file(&staged);
            return Err(e);
        }
        next.path = self.path.clone();
        next.limit = self.limit;
        *self = next;
        Ok(true)
    }

    /// [`Self::write`], then [`Self::rotate_if_full`].
    fn write_rotating(&mut self, b: &[u8]) -> std::io::Result<bool> {
        self.write(b)?;
        self.rotate_if_full()
            .map_err(|e| std::io::Error::new(e.kind(), format!("rotating it: {e}")))
    }
}

/// Write-locks the detailed log `f` at `path` (a no-op on one this process holds locked
/// already). A refused lock is said on stderr: the log is written all the same, only its
/// follower is misled.
fn write_lock(f: &std::fs::File, path: &Path) {
    let fl = libc::flock {
        l_type: libc::F_WRLCK as _,
        l_whence: libc::SEEK_SET as _,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    // SAFETY: F_SETLK on the open log takes a pointer to an initialized flock (fcntl(2)).
    if unsafe { libc::fcntl(f.as_raw_fd(), libc::F_SETLK, &fl) } != 0 {
        elog::eprintln!(
            "rusty-impulse: cannot lock the log file {}: {}; `rusty-impulse log -f` takes this instance for \
             ended",
            path.display(),
            std::io::Error::last_os_error()
        );
    }
}

fn console_sink(level: u8, line: &str) {
    let ty = match level {
        1 => Level::Info,
        2 | qml::builtins::QML_WARNING => Level::Warn,
        3 => Level::Critical,
        _ => Level::Debug,
    };
    log(ty, "qml", line);
}

/// Category filters of this process: Qt's defaults with `QT_LOGGING_RULES` applied.
fn process_filter(rules: &[Rule], category: &[u8]) -> Filter {
    let mut f = Filter::default();
    for r in rules {
        f.apply(category, r);
    }
    f
}

/// Start instance logging like `ThreadLogging::initFs`: `run_dir/log.qslog` (write-locked so
/// `qs log -f` knows when this process ends) and `run_dir/log.log`, written by a `qs-log`
/// thread; QML console output goes there under the `qml` category.
///
/// With `resumed` this image is what a reload executed and goes on as the instance of
/// `run_dir`: both files continue where the previous image left them, as Quickshell's do
/// across a reload. `inherited` is the detailed log that image kept open and locked.
pub fn start_instance_log(
    config: &str,
    shell_id: &str,
    path_id: &str,
    run_dir: &Path,
    resumed: bool,
    inherited: Option<Inherited>,
) {
    let (rules, _) = parse_rules(
        &std::env::var("QT_LOGGING_RULES")
            .unwrap_or_default()
            .replace('\n', ";"),
    );
    let (tx, rx) = mpsc::channel::<Job>();
    if SINK.set(tx).is_err() {
        return;
    }
    let _ = qml::builtins::CONSOLE_SINK.set(console_sink);
    let detailed_path = run_dir.join("log.qslog");
    let dir_ok = std::fs::create_dir_all(run_dir).is_ok();
    let open = |name: &str, detailed| {
        LogFile::open(run_dir.join(name), detailed)
            .ok()
            .filter(|_| dir_ok)
    };
    let (text, detailed, enc) = if resumed {
        log(Level::Info, "default", "Reloading configuration...");
        let text = LogFile::append(run_dir.join("log.log"));
        let file = inherited.map(|i| i.0).or_else(|| {
            let mut o = std::fs::OpenOptions::new();
            o.read(true).write(true).open(&detailed_path).ok()
        });
        match file.and_then(|f| LogFile::resume(detailed_path, f)) {
            Some((d, enc)) => (text, Some(d), enc),
            None => (text, open("log.qslog", true), Encoder::default()),
        }
    } else {
        log(
            Level::Info,
            "default",
            &format!("Launching config: {}", quote(config)),
        );
        log(
            Level::Info,
            "default",
            &format!("Shell ID: {} Path ID {}", quote(shell_id), quote(path_id)),
        );
        let text = open("log.log", false);
        if text.is_some() {
            log(
                Level::Info,
                "default",
                &format!("Saving logs to {}", quote(&detailed_path.to_string_lossy())),
            );
        }
        (text, open("log.qslog", true), Encoder::default())
    };
    let spawned = std::thread::Builder::new()
        .name("qs-log".into())
        .spawn(move || {
            let _ = WRITER.set(std::thread::current().id());
            write_logs(rx, text, detailed, enc, &rules)
        });
    if let Err(e) = spawned {
        // Refused by the system (`EAGAIN`): the files are closed with the closure, and what
        // is logged from here on is dropped.
        elog::eprintln!(
            "rusty-impulse: cannot start the qs-log thread: {e}; this instance's log is not written"
        );
    }
}

/// The `qs-log` thread: writes each message to both files, flushing whenever the queue runs
/// empty, until [`close`].
fn write_logs(
    rx: mpsc::Receiver<Job>,
    mut text: Option<LogFile>,
    mut detailed: Option<LogFile>,
    mut enc: Encoder,
    rules: &[Rule],
) {
    let mut shown: HashMap<Vec<u8>, Filter> = HashMap::new();
    let mut b = Vec::new();
    while let Ok(mut job) = rx.recv() {
        loop {
            let e = match job {
                Job::Entry(e) => e,
                Job::Flush(done) => {
                    flush_logs([&mut text, &mut detailed]);
                    let _ = done.send(());
                    break;
                }
                Job::Close(done) => {
                    flush_logs([&mut text, &mut detailed]);
                    let _ = done.send(());
                    return;
                }
                Job::KeepOpen(keep, done) => {
                    for f in [&mut text, &mut detailed].into_iter().flatten() {
                        let _ = f.out.flush();
                    }
                    let flags = if keep { 0 } else { libc::FD_CLOEXEC };
                    let fd = detailed.as_ref().map(|d| d.out.get_ref().as_raw_fd());
                    // SAFETY: fcntl(2) on the descriptor of the log this thread holds open.
                    let set = |fd: &i32| unsafe { libc::fcntl(*fd, libc::F_SETFD, flags) } == 0;
                    let _ = done.send(fd.filter(set));
                    break;
                }
            };
            let f = *shown
                .entry(e.msg.category.clone())
                .or_insert_with(|| process_filter(rules, &e.msg.category));
            if f.shows(e.msg.ty) {
                if let Some(t) = &mut text {
                    b.clear();
                    format(&mut b, &e.msg, false, Some(&local_time_ms(e.ms)), "");
                    b.push(b'\n');
                    if let Err(e) = t.write_rotating(&b) {
                        give_up(&mut text, &e);
                    }
                }
                if let Some(d) = &mut detailed {
                    b.clear();
                    enc.write(&mut b, &e.msg, e.ms, |c| process_filter(rules, c));
                    match d.write_rotating(&b) {
                        Err(e) => give_up(&mut detailed, &e),
                        // A new file: its category table and timestamps start over.
                        Ok(true) => enc = Encoder::default(),
                        Ok(false) => {}
                    }
                }
            }
            match rx.try_recv() {
                Ok(next) => job = next,
                Err(_) => break,
            }
        }
        flush_logs([&mut text, &mut detailed]);
    }
}

/// Writes out what the files buffer; one that cannot be written (a full disk) is given up.
fn flush_logs(files: [&mut Option<LogFile>; 2]) {
    for f in files {
        if let Some(Err(e)) = f.as_mut().map(|f| f.out.flush()) {
            give_up(f, &e);
        }
    }
}

/// Stops writing a log file whose write or rotation failed, and says so on stderr, the one
/// place the line can still reach: the instance log itself is what failed.
fn give_up(f: &mut Option<LogFile>, e: &std::io::Error) {
    if let Some(f) = f.take() {
        elog::eprintln!(
            "rusty-impulse: cannot write the log file {}: {e}; it is no longer written",
            f.path.display()
        );
        // What it still buffers is not written either: dropping the writer would try again.
        let (_file, _unwritten) = f.out.into_parts();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(ty: Level, cat: &str, body: &str) -> Msg {
        Msg {
            ty,
            category: cat.into(),
            body: body.into(),
        }
    }

    /// A log's time stamps are in the zone `Date` reads at that moment: the one a database
    /// under `TZDIR` holds, and after the zone changed under the running shell the new one.
    #[test]
    fn a_stamp_is_in_the_zone_the_system_has_now() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_stamp_is_in_the_zone_the_system_has_now"
        );
        if !testenv::in_own_process(TEST) {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join("Test")).unwrap();
            let zone = testenv::tzif(19020, "+0517", "<+0517>-5:17");
            std::fs::write(dir.path().join("Test/Clock"), zone).unwrap();
            let other = testenv::tzif(7200, "+02", "<+02>-2");
            std::fs::write(dir.path().join("Test/Other"), other).unwrap();
            let vars = [
                ("TZDIR", dir.path().as_os_str()),
                ("TZ", "Test/Clock".as_ref()),
            ];
            return testenv::run_in_own_process(TEST, vars);
        }
        // 2024-01-10 12:00:30 UTC.
        assert_eq!(local_time(1_704_888_030, 7), "2024-01-10 17:17:30.007");
        #[expect(
            clippy::disallowed_methods,
            reason = "the system's zone changes under a running process"
        )]
        // SAFETY: this process runs this test only, and nothing else reads its environment
        // meanwhile.
        unsafe {
            std::env::set_var("TZ", "Test/Other")
        };
        assert_eq!(local_time(1_704_888_030, 7), "2024-01-10 14:00:30.007");
        assert_eq!(local_time(i64::MAX, 0), "");
    }

    #[test]
    fn round_trips_encoded_logs() {
        let msgs = [
            (
                m(Level::Info, "default", "Launching config: \"/x\""),
                1_700_000_000_123,
            ),
            (m(Level::Debug, "qml", "hello\nwörld"), 1_700_000_000_500),
            (m(Level::Debug, "qml", "hello\nwörld"), 1_700_000_003_000),
            (
                m(Level::Warn, "qml", &"x".repeat(70_000)),
                1_700_000_040_000,
            ),
            (m(Level::Debug, "qml", "hello\nwörld"), 1_700_000_090_000),
            (
                m(Level::Critical, "quickshell.x", "e"),
                1_700_000_090_000 + 0xfff_f000,
            ),
            (m(Level::Fatal, "qml", "boom"), 1_700_100_000_000),
        ];
        let mut b = Vec::new();
        Encoder::header(&mut b);
        let mut e = Encoder::default();
        for (msg, ms) in &msgs {
            e.write(&mut b, msg, *ms, |_| Filter::default());
        }
        let recs = decode_all(&b).expect("decodes");
        assert_eq!(recs.len(), msgs.len());
        for (r, (msg, ms)) in recs.iter().zip(&msgs) {
            assert_eq!(&r.msg, msg);
            if msg.ty != Level::Fatal && !msg.category.starts_with(b"quickshell") {
                assert_eq!(r.secs, ms / 1000);
            }
        }
    }

    #[test]
    fn rules_follow_qt() {
        let (rules, bad) =
            parse_rules(" qml.debug = false ;*.warning=false;a*b=true;x=y=z;foo=maybe;none");
        assert_eq!(rules.len(), 2);
        assert_eq!(bad, ["a*b=true", "x=y=z", "foo=maybe"]);
        let mut f = Filter::default();
        for r in &rules {
            f.apply(b"qml", r);
        }
        assert_eq!(
            f,
            Filter {
                debug: false,
                info: true,
                warn: false,
                critical: true
            }
        );
    }

    /// What a reload's next image appends to the detailed log reads as one log: a category
    /// registered and a message written before the reload are referred to, not repeated.
    #[test]
    fn resumed_encoder_continues_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.qslog");
        let mut first = LogFile::open(path.clone(), true).unwrap();
        let mut enc = Encoder::default();
        let mut b = Vec::new();
        let before = [
            (
                m(Level::Info, "default", "Launching config"),
                1_700_000_000_000,
            ),
            (m(Level::Debug, "qml", "tick"), 1_700_000_001_000),
        ];
        for (msg, ms) in &before {
            enc.write(&mut b, msg, *ms, |_| Filter::default());
        }
        first.write(&b).unwrap();
        // The descriptor an exec leaves open, as `keep_across_exec` hands it on.
        let file = first.out.into_inner().unwrap();
        let (mut next, mut enc) = LogFile::resume(path.clone(), file).expect("resumes");
        let after = [
            (m(Level::Debug, "qml", "tick"), 1_700_000_005_000),
            (m(Level::Warn, "qml", "after the reload"), 1_700_000_006_000),
        ];
        let mut b = Vec::new();
        for (msg, ms) in &after {
            enc.write(&mut b, msg, *ms, |_| Filter::default());
        }
        assert_eq!(
            b[0], OP_RECENT_SHORT as u8,
            "the repeated message was written out"
        );
        assert!(
            !b.windows(3).any(|w| w == b"qml"),
            "the category was registered again"
        );
        next.write(&b).unwrap();
        next.out.flush().unwrap();
        let recs = decode_all(&std::fs::read(&path).unwrap()).expect("decodes");
        let all: Vec<_> = before.iter().chain(&after).collect();
        assert_eq!(recs.len(), all.len());
        for (r, (msg, ms)) in recs.iter().zip(all) {
            assert_eq!((&r.msg, r.secs), (msg, ms / 1000));
        }
        // A file cut off inside a message is not continued.
        let cut = std::fs::read(&path).unwrap();
        std::fs::write(&path, &cut[..cut.len() - 3]).unwrap();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(LogFile::resume(path, file).is_none());
    }

    /// Messages are flushed once the queue runs empty, `close` waits for them, and a file past
    /// its size limit is rotated to `<name>.1` with a fresh, decodable `.qslog` behind it.
    #[test]
    fn log_files_flush_rotate_and_close() {
        let dir = tempfile::tempdir().unwrap();
        let mut text = LogFile::open(dir.path().join("log.log"), false).unwrap();
        let mut detailed = LogFile::open(dir.path().join("log.qslog"), true).unwrap();
        text.limit = 4096;
        detailed.limit = 4096;
        let (tx, rx) = mpsc::channel();
        let t = std::thread::spawn(move || {
            write_logs(rx, Some(text), Some(detailed), Encoder::default(), &[])
        });
        let entry = |i: usize| {
            Job::Entry(Entry {
                msg: m(Level::Info, "qml", &format!("line {i} {}", "x".repeat(100))),
                ms: 1_700_000_000_000 + i as i64,
            })
        };
        tx.send(entry(0)).unwrap();
        let read = |n: &str| std::fs::read(dir.path().join(n)).unwrap_or_default();
        // Woken by each write to the directory's files: an idle queue flushes the line.
        testenv::DirWatch::new(dir.path())
            .until(|| String::from_utf8_lossy(&read("log.log")).contains("line 0"));
        for i in 1..100 {
            tx.send(entry(i)).unwrap();
        }
        close_sink(&tx);
        t.join().unwrap();
        let (cur, old) = (read("log.log"), read("log.log.1"));
        assert!(!old.is_empty(), "log.log was not rotated");
        for staged in ["log.log.new", "log.qslog.new"] {
            assert!(!dir.path().join(staged).exists(), "{staged} left behind");
        }
        assert!(cur.len() < 4096 + 256 && old.len() < 4096 + 256);
        assert!(
            String::from_utf8_lossy(&cur).contains("line 99"),
            "close lost the tail"
        );
        for name in ["log.qslog", "log.qslog.1"] {
            let recs = decode_all(&read(name)).expect("decodes");
            assert!(!recs.is_empty(), "{name} empty");
        }
        let last = decode_all(&read("log.qslog")).unwrap();
        assert!(String::from_utf8_lossy(&last.last().unwrap().msg.body).starts_with("line 99"));
    }

    fn open_descriptors() -> usize {
        std::fs::read_dir("/proc/self/fd").unwrap().count()
    }

    const PLAIN: Out = Out {
        color: false,
        timestamps: false,
        prefix: "",
    };

    /// A log file with its header and nothing else.
    fn empty_log(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("log.qslog");
        let mut b = Vec::new();
        Encoder::header(&mut b);
        std::fs::write(&path, b).unwrap();
        path
    }

    /// The system refuses the threads of `log -f` (the limit on threads is reached): the
    /// command fails and says why, instead of failing silently, and neither the inotify
    /// descriptor nor the copy of the log's descriptor stays open. In a process of its own:
    /// the test reads what that process wrote to its stderr and counts its open descriptors.
    #[test]
    fn refused_follow_threads_are_logged_and_leave_no_descriptor() {
        const TEST: &str = concat!(
            module_path!(),
            "::refused_follow_threads_are_logged_and_leave_no_descriptor"
        );
        if !testenv::in_own_process(TEST) {
            testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = empty_log(dir.path());
        testenv::refuse_new_threads();
        let before = open_descriptors();
        let (ok, text) = testenv::stderr_with(|| {
            let file = std::fs::File::open(&path).unwrap();
            read_logs(file, &path, &PLAIN, 0, true, "")
        });
        assert!(!ok);
        assert_eq!(open_descriptors(), before, "open descriptors");
        let watch = format!(
            "rusty-impulse log: cannot watch {} for changes:",
            path.display()
        );
        assert!(text.contains(&watch), "stderr: {text:?}");
        assert!(
            text.contains("rusty-impulse log: cannot wait for the log's writer to let go of it:"),
            "stderr: {text:?}"
        );
    }

    /// inotify cannot watch the log (it was removed after `log -f` opened it): the command
    /// says so instead of silently showing nothing new, and still ends once no writer holds
    /// the file. In a process of its own: the test reads what that process wrote to its
    /// stderr.
    #[test]
    fn an_unwatchable_log_is_reported() {
        const TEST: &str = concat!(module_path!(), "::an_unwatchable_log_is_reported");
        if !testenv::in_own_process(TEST) {
            testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = empty_log(dir.path());
        let file = std::fs::File::open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let (ok, text) = testenv::stderr_with(|| read_logs(file, &path, &PLAIN, 0, true, ""));
        assert!(ok);
        let watch = format!(
            "rusty-impulse log: cannot watch {} for changes:",
            path.display()
        );
        assert!(text.contains(&watch), "stderr: {text:?}");
    }

    /// The system refuses the `qs-log` thread (the limit on threads is reached as the shell
    /// starts): the shell says that its instance log is not written, the two files are closed,
    /// and logging and `close` return. In a process of its own: instance logging starts once
    /// per process, and the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_writer_thread_is_logged() {
        const TEST: &str = concat!(module_path!(), "::a_refused_writer_thread_is_logged");
        if !testenv::in_own_process(TEST) {
            testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        testenv::refuse_new_threads();
        let before = open_descriptors();
        let ((), text) = testenv::stderr_with(|| {
            start_instance_log("shell.qml", "id", "id", dir.path(), false, None)
        });
        assert!(
            text.contains("rusty-impulse: cannot start the qs-log thread:"),
            "stderr: {text:?}"
        );
        assert_eq!(open_descriptors(), before, "open descriptors");
        log(Level::Info, "qml", "dropped");
        close();
    }

    /// A log file that cannot be written (`/dev/full`: every write fails with `ENOSPC`, as on
    /// a full disk) is given up with a line on stderr instead of silently; the other file is
    /// written on. In a process of its own: the test reads what that process wrote to its
    /// stderr.
    #[test]
    fn a_failed_write_is_reported_and_the_other_file_goes_on() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_failed_write_is_reported_and_the_other_file_goes_on"
        );
        if !testenv::in_own_process(TEST) {
            testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let text = LogFile::open("/dev/full".into(), false).unwrap();
        let detailed = LogFile::open(dir.path().join("log.qslog"), true).unwrap();
        let (tx, rx) = mpsc::channel();
        let t = std::thread::spawn(move || {
            write_logs(rx, Some(text), Some(detailed), Encoder::default(), &[])
        });
        let entry = |i: usize| {
            Job::Entry(Entry {
                msg: m(Level::Info, "qml", &format!("line {i}")),
                ms: 1_700_000_000_000 + i as i64,
            })
        };
        let ((), said) = testenv::stderr_with(|| {
            tx.send(entry(0)).unwrap();
            flush_sink(&tx);
            tx.send(entry(1)).unwrap();
            close_sink(&tx);
            t.join().unwrap();
        });
        assert_eq!(
            said.matches("rusty-impulse: cannot write the log file /dev/full:")
                .count(),
            1,
            "stderr: {said:?}"
        );
        let recs = decode_all(&std::fs::read(dir.path().join("log.qslog")).unwrap()).unwrap();
        let bodies: Vec<_> = recs.iter().map(|r| r.msg.body.clone()).collect();
        assert_eq!(bodies, [b"line 0".to_vec(), b"line 1".to_vec()]);
    }

    /// A log file that cannot be rotated (here a directory is in the way of its successor) is
    /// given up with a line on stderr instead of silently. In a process of its own: the test
    /// reads what that process wrote to its stderr.
    #[test]
    fn a_failed_rotation_is_reported() {
        const TEST: &str = concat!(module_path!(), "::a_failed_rotation_is_reported");
        if !testenv::in_own_process(TEST) {
            testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.log");
        let mut text = LogFile::open(path.clone(), false).unwrap();
        text.limit = 1;
        std::fs::create_dir(dir.path().join("log.log.new")).unwrap();
        let (tx, rx) = mpsc::channel();
        let t =
            std::thread::spawn(move || write_logs(rx, Some(text), None, Encoder::default(), &[]));
        let ((), said) = testenv::stderr_with(|| {
            for body in ["first", "second"] {
                tx.send(Job::Entry(Entry {
                    msg: m(Level::Info, "qml", body),
                    ms: 1_700_000_000_000,
                }))
                .unwrap();
            }
            close_sink(&tx);
            t.join().unwrap();
        });
        let line = format!(
            "rusty-impulse: cannot write the log file {}: rotating it:",
            path.display()
        );
        assert_eq!(said.matches(&line).count(), 1, "stderr: {said:?}");
    }

    /// Another open file description holds a lock on the log (`F_OFD_SETLK`, which conflicts
    /// with the writer's `F_SETLK` in the same process too): the writer says that it could not
    /// lock the file, which `log -f` reads as an ended instance, and writes the log all the
    /// same. In a process of its own: the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_file_lock_is_reported() {
        const TEST: &str = concat!(module_path!(), "::a_refused_file_lock_is_reported");
        if !testenv::in_own_process(TEST) {
            testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.qslog");
        let holder = std::fs::File::create(&path).unwrap();
        let fl = libc::flock {
            l_type: libc::F_WRLCK as _,
            l_whence: libc::SEEK_SET as _,
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        };
        // SAFETY: F_OFD_SETLK on the open file takes a pointer to an initialized flock with
        // `l_pid` 0 (fcntl(2)).
        let locked = unsafe { libc::fcntl(holder.as_raw_fd(), libc::F_OFD_SETLK, &fl) };
        assert_eq!(locked, 0, "{}", std::io::Error::last_os_error());
        let (log, said) = testenv::stderr_with(|| LogFile::open(path.clone(), true));
        let line = format!(
            "rusty-impulse: cannot lock the log file {}:",
            path.display()
        );
        assert!(said.contains(&line), "stderr: {said:?}");
        let mut log = log.unwrap();
        log.out.flush().unwrap();
        let written = decode_all(&std::fs::read(&path).unwrap());
        assert_eq!(written.map(|recs| recs.len()), Some(0));
    }

    /// `flush` (before `Rustshell.reload()`'s exec) returns only once every line queued before
    /// it is in both files, and the writer goes on logging after it.
    #[test]
    fn flush_writes_the_queue_and_keeps_logging() {
        let dir = tempfile::tempdir().unwrap();
        let text = LogFile::open(dir.path().join("log.log"), false).unwrap();
        let detailed = LogFile::open(dir.path().join("log.qslog"), true).unwrap();
        let (tx, rx) = mpsc::channel();
        let t = std::thread::spawn(move || {
            write_logs(rx, Some(text), Some(detailed), Encoder::default(), &[])
        });
        let entry = |i: usize| {
            Job::Entry(Entry {
                msg: m(Level::Info, "qml", &format!("line {i}")),
                ms: 1_700_000_000_000 + i as i64,
            })
        };
        let read = |n: &str| std::fs::read(dir.path().join(n)).unwrap_or_default();
        let lines = |b: &[u8]| String::from_utf8_lossy(b).lines().count();
        for i in 0..2000 {
            tx.send(entry(i)).unwrap();
        }
        flush_sink(&tx);
        let text = read("log.log");
        assert_eq!(
            lines(&text),
            2000,
            "flush returned before the queue was written"
        );
        assert!(String::from_utf8_lossy(&text).ends_with("line 1999\n"));
        let last = decode_all(&read("log.qslog")).expect("decodes");
        assert_eq!(last.len(), 2000);
        tx.send(entry(2000)).unwrap();
        close_sink(&tx);
        t.join().unwrap();
        assert!(
            String::from_utf8_lossy(&read("log.log")).ends_with("line 2000\n"),
            "the writer stopped at the flush"
        );
    }

    /// A panic of the `qs-log` thread does not wait for that thread to flush: here nobody reads
    /// the queue, as while the thread is in its panic hook, and the flush returns.
    #[test]
    fn the_writer_does_not_wait_for_itself_in_a_panic() {
        let (tx, _unread) = mpsc::channel();
        SINK.set(tx).expect("this process logs nowhere else");
        WRITER
            .set(std::thread::current().id())
            .expect("this process has no qs-log thread");
        flush_in_panic();
    }

    /// `close` waits for the writer however long the backlog takes: here `log.log` is a FIFO
    /// nobody reads until `close` is under way, so the writer blocks with most of the log still
    /// queued, and `close` cannot have returned while the reader is held. Once `close`
    /// returns, every line must be in the pipe or read.
    #[test]
    fn close_waits_for_a_slow_writer() {
        use std::sync::{Arc, Mutex};
        const LINES: usize = 4000;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.log");
        let cpath = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `cpath` is a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        let text = LogFile::open(path.clone(), false).unwrap();
        // The log holds the FIFO open read-write, so this open does not block (fifo(7)).
        let mut fifo = std::fs::File::open(&path).unwrap();
        let mut pending = fifo.try_clone().unwrap();
        let got = Arc::new(Mutex::new(Vec::new()));
        let (go, wait) = mpsc::channel::<()>();
        let reader = std::thread::spawn({
            let got = got.clone();
            move || {
                // Held like a stalled disk until the test releases it (or gives up: dropped).
                let _ = wait.recv();
                let mut buf = [0u8; 4096];
                loop {
                    let mut got = got.lock().unwrap();
                    match fifo.read(&mut buf).unwrap() {
                        0 => break,
                        n => got.extend_from_slice(&buf[..n]),
                    }
                }
            }
        });
        let (tx, rx) = mpsc::channel();
        let writer =
            std::thread::spawn(move || write_logs(rx, Some(text), None, Encoder::default(), &[]));
        for i in 0..LINES {
            tx.send(Job::Entry(Entry {
                msg: m(Level::Info, "qml", &format!("line {i} {}", "x".repeat(100))),
                ms: 1_700_000_000_000 + i as i64,
            }))
            .unwrap();
        }
        let (entered, closing) = mpsc::channel();
        let (returned, closed) = mpsc::channel();
        let closer = std::thread::spawn(move || {
            entered.send(()).unwrap();
            close_sink(&tx);
            returned.send(()).unwrap();
        });
        closing.recv().unwrap();
        // The FIFO holds far less than the log and nothing reads it: `close` is still waiting.
        assert!(
            closed.try_recv().is_err(),
            "close returned with the writer blocked on a stalled reader"
        );
        go.send(()).unwrap();
        closed.recv().unwrap();
        closer.join().unwrap();
        let mut all = got.lock().unwrap();
        let mut queued: libc::c_int = 0;
        // SAFETY: FIONREAD on the FIFO stores the byte count in the int passed (ioctl(2)).
        let rc = unsafe { libc::ioctl(pending.as_raw_fd(), libc::FIONREAD, &mut queued) };
        assert_eq!(rc, 0);
        let start = all.len();
        all.resize(start + usize::try_from(queued).unwrap(), 0);
        pending.read_exact(&mut all[start..]).unwrap();
        let text = String::from_utf8_lossy(&all).into_owned();
        drop(all);
        assert!(
            text.contains(&format!("line {} ", LINES - 1)),
            "close returned before the tail was written ({} of the log's lines)",
            text.lines().count()
        );
        assert_eq!(text.lines().count(), LINES);
        writer.join().unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn quotes_like_qdebug() {
        assert_eq!(
            quote_bytes(b"\x01a\x02g\"\\\n"),
            "\"\\x01\"\"a\\x02g\\\"\\\\\\n\""
        );
        assert_eq!(quote("a\"b\u{1}"), "\"a\\\"b\\u0001\"");
    }
}
