//! The text of an XMLHttpRequest reply: QQmlXMLHttpRequest::findTextDecoder and the
//! QStringDecoder it picks.

use encoding_rs::Encoding;

/// A decoder QStringDecoder has for a charset name: one of Qt's own, or a converter of the
/// ICU that Qt is built with.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Charset {
    Utf8,
    /// Little-endian unless the text starts with the other byte order mark.
    Utf16,
    Utf16Le,
    Utf16Be,
    Utf32,
    Utf32Le,
    Utf32Be,
    /// ISO-8859-1 itself: every byte is its code point.
    Latin1,
    Ascii,
    /// ICU's UTF-8, and with `true` its CESU-8.
    IcuUtf8(bool),
    /// ICU's UTF-16 and UTF-32: big-endian with `Some(true)`, little-endian with
    /// `Some(false)`, and without either as the mark at the start says, big-endian
    /// without a mark.
    IcuUtf16(Option<bool>),
    IcuUtf32(Option<bool>),
    /// A charset of one byte for a character.
    Single(&'static Encoding, Bytes),
    Multi(Multi),
    Iso2022Jp,
    /// A converter ICU has and this runtime has not.
    NotWritten,
}

/// The single bytes that ICU's table of a charset reads otherwise than the table
/// `encoding_rs` has for it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Bytes {
    Same,
    /// 0x80 to 0x9F are U+0080 to U+009F: ISO-8859-9 and ISO-8859-11, read with the
    /// tables of windows-1254 and windows-874.
    C1,
    Windows874,
    Ibm866,
    Koi8U,
    Windows1253,
    Windows1255,
}

impl Bytes {
    /// The bytes and what ICU reads them as.
    fn of_icu(self) -> Vec<(u8, char)> {
        let run = |from: u8, to: u8, first: u32| {
            (from..=to).map(move |b| (b, char::from_u32(first + u32::from(b - from)).unwrap()))
        };
        match self {
            Bytes::Same => vec![],
            Bytes::C1 => run(0x80, 0x9F, 0x80).collect(),
            Bytes::Windows874 => run(0xDB, 0xDE, 0xF8C1)
                .chain(run(0xFC, 0xFF, 0xF8C5))
                .collect(),
            Bytes::Ibm866 => vec![(0x1A, '\u{1C}'), (0x1C, '\u{7F}'), (0x7F, '\u{1A}')],
            Bytes::Koi8U => vec![(0xAE, '\u{255D}'), (0xBE, '\u{256C}')],
            Bytes::Windows1253 => vec![(0xAA, '\u{AA}')],
            Bytes::Windows1255 => vec![(0xCA, '\u{FFFD}')],
        }
    }
}

/// A charset of more bytes than one for a character that ICU reads with a table.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Multi {
    ShiftJis,
    EucJp,
    Big5,
    Big5Hkscs,
    Gbk,
    Gb2312,
    Gb18030,
    EucKr,
    Windows949,
}

/// The characters of a charset that ICU's table has otherwise than the table of
/// `encoding_rs` (xhr_charset_data.rs, made from ICU by tools/gen/charset/gen.py): the bytes
/// of a character as one number, the first byte highest, and what ICU reads them as,
/// U+FFFD where it has no character for them. Both lists go up by the bytes.
pub(crate) struct Fixes {
    /// Characters of at most two bytes that are read as one unit of UTF-16.
    pub(crate) two: &'static [(u16, u16)],
    pub(crate) more: &'static [(u32, char)],
}

impl Fixes {
    fn of(&self, bytes: &[u8]) -> Option<char> {
        let key = bytes.iter().fold(0, |key, &b| key << 8 | u32::from(b));
        if let Ok(key) = u16::try_from(key)
            && let Ok(at) = self.two.binary_search_by_key(&key, |fix| fix.0)
        {
            return char::from_u32(self.two[at].1.into());
        }
        let at = self.more.binary_search_by_key(&key, |fix| fix.0).ok()?;
        Some(self.more[at].1)
    }
}

/// What a single byte is in a charset of more bytes than one.
enum Byte {
    Is(char),
    /// A byte of its own that stands for nothing.
    Nothing,
    /// What the table of `encoding_rs` says.
    Table,
}

impl Multi {
    /// Which bytes make a character: the lines `<icu:state>` in the header of ICU's table,
    /// as they are written there. A line is a state, the first is where a character
    /// starts. `81-9f:1` sends the bytes 0x81 to 0x9F on to state 1, a range without a
    /// colon ends a character, and a byte in no range of its state cannot be there.
    fn states(self) -> &'static [&'static str] {
        match self {
            // ibm-943_P15A-2003
            Multi::ShiftJis => &["0-80, 81-9f:1, a0-df, e0-fc:1, fd-ff", "40-7e, 80-fc"],
            // euc-jp-2007
            Multi::EucJp => &[
                "0-8d, 8e:2, 8f:3, 90-9f, a1-fe:1",
                "a1-fe",
                "a1-e4",
                "a1-fe:1, a1:4, a3-a4:4, ef-f2:4, f4-fe:4",
                "a1-fe.u",
            ],
            // windows-950-2000, windows-936-2000, windows-949-2000
            Multi::Big5 | Multi::Gbk | Multi::Windows949 => &["0-80, 81-fe:1, ff", "40-7e, 80-fe"],
            // ibm-1375_P100-2008
            Multi::Big5Hkscs => &[
                "0-7f, 81-fe:1, 87-a0:2, c8:2, fa-fe:2",
                "40-7e, a1-fe",
                "40-7e.p, a1-fe.p",
            ],
            // ibm-1383_P110-1999, ibm-970_P110_P110-2006_U2
            Multi::Gb2312 | Multi::EucKr => &["0-9f, a1-fe:1", "a1-fe"],
            // gb18030
            Multi::Gb18030 => &[
                "0-7f, 81:6, 82:7, 83:8, 84:9, 85-fe:3",
                "81-fe:2",
                "30-39",
                "30-39:4, 40-7e, 80-fe",
                "81-fe:5",
                "30-39.u",
                "30:1, 31-34:4, 35:a, 36-39:1, 40-7e, 80-fe",
                "30-35:1, 36-39:4, 40-7e, 80-fe",
                "30-35:4, 36:1, 37-39:4, 40-7e, 80-fe",
                "30-31:1, 32-39:4, 40-7e, 80-fe",
                "81-fe:5, f4:2",
            ],
        }
    }

    /// Whether one byte that is no character becomes U+001A (the table has a `<subchar1>`)
    /// and not U+FFFD.
    fn one_byte_is_sub(self) -> bool {
        matches!(
            self,
            Multi::ShiftJis | Multi::EucJp | Multi::Gb2312 | Multi::EucKr
        )
    }

    /// The table of `encoding_rs` that reads the characters.
    fn table(self) -> &'static Encoding {
        match self {
            Multi::ShiftJis => encoding_rs::SHIFT_JIS,
            Multi::EucJp => encoding_rs::EUC_JP,
            Multi::Big5 | Multi::Big5Hkscs => encoding_rs::BIG5,
            Multi::Gbk | Multi::Gb2312 => encoding_rs::GBK,
            Multi::Gb18030 => encoding_rs::GB18030,
            Multi::EucKr | Multi::Windows949 => encoding_rs::EUC_KR,
        }
    }

    /// The characters ICU's table has otherwise than the table of `encoding_rs`.
    fn fixes(self) -> &'static Fixes {
        use crate::xhr_charset_data as data;
        match self {
            Multi::ShiftJis => &data::SHIFT_JIS,
            Multi::EucJp => &data::EUC_JP,
            Multi::Big5 => &data::BIG5,
            Multi::Big5Hkscs => &data::BIG5_HKSCS,
            Multi::Gbk => &data::GBK,
            Multi::Gb2312 => &data::GB2312,
            Multi::Gb18030 => &data::GB18030,
            Multi::EucKr => &data::EUC_KR,
            Multi::Windows949 => &data::WINDOWS_949,
        }
    }

    /// The single bytes ICU's table has otherwise than the table of `encoding_rs`.
    fn byte(self, b: u8) -> Byte {
        match (self, b) {
            (Multi::ShiftJis, 0x1A) => Byte::Is('\u{1C}'),
            (Multi::ShiftJis, 0x1C) => Byte::Is('\u{7F}'),
            (Multi::ShiftJis, 0x7F) => Byte::Is('\u{1A}'),
            (Multi::ShiftJis, 0x80) => Byte::Nothing,
            (Multi::Gb2312 | Multi::EucKr, 0x8E | 0x8F) => Byte::Nothing,
            (Multi::EucJp | Multi::Gb2312 | Multi::EucKr, 0x80..=0x9F) => Byte::Is(b.into()),
            (Multi::Big5 | Multi::Windows949, 0x80) => Byte::Is('\u{80}'),
            (Multi::Big5, 0xFF) => Byte::Is('\u{F8F8}'),
            (Multi::Gbk, 0xFF) => Byte::Is('\u{F8F5}'),
            (Multi::Windows949, 0xFF) => Byte::Is('\u{F8F7}'),
            _ => Byte::Table,
        }
    }
}

/// QStringConverter::encodingForName: the decoders Qt has itself, by their names without
/// case and without `-` and `_`. `Locale` is the encoding of the system, UTF-8.
fn own(name: &[u8]) -> Option<Charset> {
    let key: Vec<u8> = name
        .iter()
        .filter(|b| !matches!(b, b'-' | b'_'))
        .map(u8::to_ascii_lowercase)
        .collect();
    Some(match &key[..] {
        b"utf8" | b"locale" => Charset::Utf8,
        b"utf16" => Charset::Utf16,
        b"utf16le" => Charset::Utf16Le,
        b"utf16be" => Charset::Utf16Be,
        b"utf32" => Charset::Utf32,
        b"utf32le" => Charset::Utf32Le,
        b"utf32be" => Charset::Utf32Be,
        b"iso88591" | b"latin1" => Charset::Latin1,
        _ => return None,
    })
}

/// A name as ICU compares the names of converters (ucnv_io_stripASCIIForCompare): its
/// letters in small case and its digits, without the zeros a number starts with.
fn icu_key(name: &[u8]) -> String {
    let mut key = String::new();
    let mut after_digit = false;
    for (i, b) in name.iter().enumerate() {
        match b {
            b'0' if !after_digit && name.get(i + 1).is_some_and(u8::is_ascii_digit) => continue,
            b'0' => {}
            b'1'..=b'9' => after_digit = true,
            b if b.is_ascii_alphabetic() => after_digit = false,
            _ => {
                after_digit = false;
                continue;
            }
        }
        key.push(b.to_ascii_lowercase().into());
    }
    key
}

/// Every name the ICU of Qt 6.12 opens a converter by: the converter, a tab, the name.
/// `crates/qml/tests/qt_icu.py names` writes it, and the test
/// `every_charset_name_of_qt_is_read_as_qt_or_is_not_written` holds it to what ICU says.
const ICU_NAMES: &str = include_str!("xhr_charsets.tsv");

/// The decoder for a converter of ICU, by the name ICU has for the converter itself.
fn converter(of: &str) -> Charset {
    use Bytes::{C1, Same};
    use Charset::{IcuUtf8, IcuUtf16, IcuUtf32, Single};
    use encoding_rs as e;
    match of {
        "UTF-8" => IcuUtf8(false),
        "CESU-8" => IcuUtf8(true),
        "UTF-16" => IcuUtf16(None),
        "UTF-16BE" => IcuUtf16(Some(true)),
        "UTF-16LE" => IcuUtf16(Some(false)),
        "UTF-32" => IcuUtf32(None),
        "UTF-32BE" => IcuUtf32(Some(true)),
        "UTF-32LE" => IcuUtf32(Some(false)),
        "ISO-8859-1" => Charset::Latin1,
        "US-ASCII" => Charset::Ascii,
        "ibm-912_P100-1995" => Single(e::ISO_8859_2, Same),
        "ibm-913_P100-2000" => Single(e::ISO_8859_3, Same),
        "ibm-914_P100-1995" => Single(e::ISO_8859_4, Same),
        "ibm-915_P100-1995" => Single(e::ISO_8859_5, Same),
        "ibm-1089_P100-1995" => Single(e::ISO_8859_6, Same),
        "ibm-9005_X110-2007" => Single(e::ISO_8859_7, Same),
        "ibm-5012_P100-1999" => Single(e::ISO_8859_8, Same),
        "ibm-920_P100-1995" => Single(e::WINDOWS_1254, C1),
        "iso-8859_10-1998" => Single(e::ISO_8859_10, Same),
        "iso-8859_11-2001" => Single(e::WINDOWS_874, C1),
        "ibm-921_P100-1995" => Single(e::ISO_8859_13, Same),
        "iso-8859_14-1998" => Single(e::ISO_8859_14, Same),
        "ibm-923_P100-1998" => Single(e::ISO_8859_15, Same),
        "ibm-878_P100-1996" => Single(e::KOI8_R, Same),
        "ibm-1168_P100-2002" => Single(e::KOI8_U, Bytes::Koi8U),
        "ibm-866_P100-1995" => Single(e::IBM866, Bytes::Ibm866),
        "ibm-5346_P100-1998" => Single(e::WINDOWS_1250, Same),
        "ibm-5347_P100-1998" => Single(e::WINDOWS_1251, Same),
        "ibm-5348_P100-1997" => Single(e::WINDOWS_1252, Same),
        "ibm-5349_P100-1998" => Single(e::WINDOWS_1253, Bytes::Windows1253),
        "ibm-5350_P100-1998" => Single(e::WINDOWS_1254, Same),
        "ibm-9447_P100-2002" => Single(e::WINDOWS_1255, Bytes::Windows1255),
        "ibm-9448_X100-2005" => Single(e::WINDOWS_1256, Same),
        "ibm-9449_P100-2002" => Single(e::WINDOWS_1257, Same),
        "ibm-5354_P100-1998" => Single(e::WINDOWS_1258, Same),
        "windows-874-2000" => Single(e::WINDOWS_874, Bytes::Windows874),
        "macos-0_2-10.2" => Single(e::MACINTOSH, Same),
        "ibm-943_P15A-2003" => Charset::Multi(Multi::ShiftJis),
        "euc-jp-2007" => Charset::Multi(Multi::EucJp),
        "windows-950-2000" => Charset::Multi(Multi::Big5),
        "ibm-1375_P100-2008" => Charset::Multi(Multi::Big5Hkscs),
        "windows-936-2000" => Charset::Multi(Multi::Gbk),
        "ibm-1383_P110-1999" => Charset::Multi(Multi::Gb2312),
        "gb18030" => Charset::Multi(Multi::Gb18030),
        "ibm-970_P110_P110-2006_U2" => Charset::Multi(Multi::EucKr),
        "windows-949-2000" => Charset::Multi(Multi::Windows949),
        "ISO_2022,locale=ja,version=0" => Charset::Iso2022Jp,
        _ => Charset::NotWritten,
    }
}

/// The decoder QStringDecoder makes for `name`: Qt's own, else the converter ICU opens by
/// that name. A name with the options ICU takes after a comma is found only as ICU itself
/// lists it. `None` for a name neither knows.
pub(crate) fn charset(name: &[u8]) -> Option<Charset> {
    static BY_NAME: std::sync::LazyLock<std::collections::HashMap<String, Charset>> =
        std::sync::LazyLock::new(|| {
            ICU_NAMES
                .lines()
                .filter_map(|l| l.split_once('\t'))
                .map(|(of, name)| (icu_key(name.as_bytes()), converter(of)))
                .collect()
        });
    own(name).or_else(|| BY_NAME.get(&icu_key(name)).copied())
}

/// QStringConverter::encodingForData: the encoding a byte order mark names.
fn marked(data: &[u8]) -> Option<Charset> {
    if data.len() > 3 {
        match data[..4] {
            [0, 0, 0xFE, 0xFF] => return Some(Charset::Utf32Be),
            [0xFF, 0xFE, 0, 0] => return Some(Charset::Utf32Le),
            _ => {}
        }
    }
    if data.len() > 2 && data[..3] == [0xEF, 0xBB, 0xBF] {
        return Some(Charset::Utf8);
    }
    if data.len() > 1 {
        match data[..2] {
            [0xFE, 0xFF] => return Some(Charset::Utf16Be),
            [0xFF, 0xFE] => return Some(Charset::Utf16Le),
            _ => {}
        }
    }
    None
}

/// The `encoding` of the XML declaration `data` starts with (QXmlStreamReader's
/// documentEncoding after the first token).
pub(crate) fn xml_encoding(data: &[u8]) -> Option<Vec<u8>> {
    let text = match marked(data) {
        Some(c @ (Charset::Utf16Le | Charset::Utf16Be)) => decode(c, data).into_bytes(),
        _ => data.to_vec(),
    };
    let text = text.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&text);
    let decl = text.strip_prefix(b"<?xml")?;
    if !decl.first()?.is_ascii_whitespace() {
        return None;
    }
    let decl = &decl[..decl.windows(2).position(|w| w == b"?>")?];
    let at = decl.windows(8).position(|w| w == b"encoding")? + 8;
    let rest = decl[at..].trim_ascii_start().strip_prefix(b"=")?;
    let rest = rest.trim_ascii_start();
    let quote = *rest.first().filter(|q| matches!(q, b'"' | b'\''))?;
    let value = &rest[1..];
    Some(value[..value.iter().position(|b| *b == quote)?].to_vec())
}

/// parseHtmlMetaForEncoding: the charset a `<meta>` in the first 1024 bytes names.
fn html_meta(data: &[u8]) -> Option<Vec<u8>> {
    let head = data[..data.len().min(1024)].to_ascii_lowercase();
    let find = |from: usize, what: &[u8]| {
        head[from..]
            .windows(what.len())
            .position(|w| w == what)
            .map(|i| from + i)
    };
    let mut pos = find(find(0, b"meta ")?, b"charset=")? + 8;
    if matches!(head.get(pos), Some(b'"' | b'\'')) {
        pos += 1;
    }
    for end in pos + 1..head.len() {
        if !matches!(head[end], b'"' | b'\'' | b'>' | b'/') {
            continue;
        }
        let mut name = &head[pos..end];
        if let Some(colon) = name.iter().position(|b| *b == b':').filter(|c| *c > 0) {
            name = &name[..colon];
        }
        // QByteArray::simplified.
        let name = name
            .split(|b| b.is_ascii_whitespace())
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(&b' ');
        if name == b"unicode" {
            return Some(b"UTF-8".to_vec());
        }
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// QQmlXMLHttpRequest::findTextDecoder: the reply's (or the overriding) charset, else the
/// XML declaration's of what is XML, else a mark or a `<meta>` of `text/html`, else a mark,
/// else UTF-8. A name whose converter is not written here counts as no name, and the first
/// such name met comes back with the decoder.
pub(crate) fn find(name: &[u8], xml: bool, mime: &[u8], data: &[u8]) -> (Charset, Option<Vec<u8>>) {
    let mut unread = None;
    let mut named = |name: &[u8]| match charset(name) {
        Some(Charset::NotWritten) => {
            unread.get_or_insert_with(|| name.to_vec());
            None
        }
        found => found,
    };
    let mut found = named(name);
    if found.is_none() && xml {
        found = xml_encoding(data).and_then(|n| named(&n));
    }
    if found.is_none() && mime == b"text/html" {
        found = marked(data).or_else(|| html_meta(data).and_then(|n| named(&n)));
    }
    (
        found.or_else(|| marked(data)).unwrap_or(Charset::Utf8),
        unread,
    )
}

/// The line of the log for a reply in the charset `name`, which Qt has a converter for and
/// this runtime has not.
pub(crate) fn unread(name: &[u8]) -> String {
    format!(
        "XMLHttpRequest: no decoder here for the charset \"{}\", which Qt reads: the reply is read as if it named none (as UTF-8, unless its text names another)",
        String::from_utf8_lossy(name)
    )
}

/// QUtf8::convertToUnicode: a byte that cannot be where it is becomes U+FFFD and decoding
/// goes on after it; a character the end of the data cuts is left out; a mark at the start
/// is dropped.
fn utf8(data: &[u8]) -> String {
    let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
    let mut out = String::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        let (len, min) = match b {
            0..0x80 => {
                out.push(b as char);
                i += 1;
                continue;
            }
            0xC2..0xE0 => (2, 0x80),
            0xE0..0xF0 => (3, 0x800),
            0xF0..0xF5 => (4, 0x1_0000),
            _ => (0, 0),
        };
        let tail = &data[i + 1..data.len().min(i + len.max(1))];
        let whole = len > 0 && tail.iter().all(|c| c & 0xC0 == 0x80);
        if whole && tail.len() + 1 < len {
            break;
        }
        let c = whole
            .then(|| {
                tail.iter().fold(u32::from(b) & (0x7F >> len), |c, t| {
                    c << 6 | u32::from(t & 0x3F)
                })
            })
            .filter(|c| *c >= min)
            .and_then(char::from_u32);
        match c {
            Some(c) => {
                out.push(c);
                i += len;
            }
            None => {
                out.push('\u{FFFD}');
                i += 1;
            }
        }
    }
    out
}

/// Qt's own UTF-16: the units as they are, a surrogate without its pair too.
fn utf16(data: &[u8], big: bool) -> Vec<u16> {
    let units = data.as_chunks::<2>().0.iter();
    units
        .map(|&p| {
            if big {
                u16::from_be_bytes(p)
            } else {
                u16::from_le_bytes(p)
            }
        })
        .collect()
}

fn utf32(data: &[u8], big: bool) -> String {
    data.as_chunks::<4>()
        .0
        .iter()
        .map(|&p| {
            let c = if big {
                u32::from_be_bytes(p)
            } else {
                u32::from_le_bytes(p)
            };
            char::from_u32(c).unwrap_or('\u{FFFD}')
        })
        .collect()
}

/// ICU's UTF-8 (ucnv_toUnicode_UTF8): the bytes of a character that cannot go on are one
/// U+FFFD together, and the byte that stopped it is read again; a character the end of
/// the data cuts is left out; a mark at the start stays. Its CESU-8 reads a surrogate in
/// three bytes as that unit, and four bytes for a character are one U+FFFD.
fn icu_utf8(data: &[u8], cesu: bool) -> Vec<u16> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while let Some(&b) = data.get(i) {
        let len = match b {
            0..0x80 => {
                out.push(b.into());
                i += 1;
                continue;
            }
            0xC2..0xE0 => 2,
            0xE0..0xF0 => 3,
            0xF0..0xF5 => 4,
            _ => 1,
        };
        let mut c = u32::from(b) & (0x7F >> len);
        let mut read = 1;
        while read < len {
            let Some(&t) = data.get(i + read) else {
                return out;
            };
            let second = match b {
                0xE0 => 0xA0..=0xBF,
                0xED if !cesu => 0x80..=0x9F,
                0xF0 => 0x90..=0xBF,
                0xF4 => 0x80..=0x8F,
                _ => 0x80..=0xBF,
            };
            if !(if read == 1 { second } else { 0x80..=0xBF }).contains(&t) {
                break;
            }
            c = c << 6 | u32::from(t & 0x3F);
            read += 1;
        }
        i += read;
        match char::from_u32(c) {
            _ if read < len || len == 1 || (cesu && len == 4) => out.push(0xFFFD),
            Some(c) => out.extend(c.encode_utf16(&mut [0; 2]).iter()),
            // A surrogate of CESU-8.
            None => out.push(c as u16),
        }
    }
    out
}

/// ICU's UTF-16 (ucnv_u16.cpp): a surrogate without its pair is U+FFFD, and a first
/// surrogate at the end waits for its second.
fn icu_utf16(data: &[u8], big: Option<bool>) -> Vec<u16> {
    let (big, data) = match (big, data) {
        (Some(big), _) => (big, data),
        (None, [0xFF, 0xFE, rest @ ..]) => (false, rest),
        (None, [0xFE, 0xFF, rest @ ..]) => (true, rest),
        (None, _) => (true, data),
    };
    let units = utf16(data, big);
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while let Some(&u) = units.get(i) {
        i += 1;
        match (u, units.get(i)) {
            (0xD800..0xDC00, None) => break,
            (0xD800..0xDC00, Some(&second @ 0xDC00..0xE000)) => {
                out.extend([u, second]);
                i += 1;
            }
            (0xD800..0xE000, _) => out.push(0xFFFD),
            _ => out.push(u),
        }
    }
    out
}

/// ICU's UTF-32 (ucnv_u32.cpp): four bytes that are no character are U+FFFD.
fn icu_utf32(data: &[u8], big: Option<bool>) -> String {
    match (big, data) {
        (Some(big), _) => utf32(data, big),
        (None, [0xFF, 0xFE, 0, 0, rest @ ..]) => utf32(rest, false),
        (None, [0, 0, 0xFE, 0xFF, rest @ ..]) => utf32(rest, true),
        (None, _) => utf32(data, true),
    }
}

/// A charset of one byte for a character: the table of `encoding_rs`, but for `but`.
fn single(table: &'static Encoding, but: Bytes, data: &[u8]) -> String {
    let every: Vec<u8> = (0..=255).collect();
    let mut read: Vec<char> = table
        .decode_without_bom_handling(&every)
        .0
        .chars()
        .collect();
    for (b, c) in but.of_icu() {
        read[usize::from(b)] = c;
    }
    data.iter().map(|b| read[usize::from(*b)]).collect()
}

/// What a byte does in a state of a charset of more bytes than one.
#[derive(Clone, Copy, PartialEq)]
enum Step {
    /// The byte cannot be here.
    No,
    /// The character goes on in this state.
    On(usize),
    /// The character ends with the byte.
    End,
}

/// The states ICU's table header writes (`Multi::states`), a step for every byte in each.
fn steps(states: &[&str]) -> Vec<[Step; 256]> {
    let hex = |s: &str| usize::from_str_radix(s, 16).unwrap();
    let state = |line: &str| {
        let mut row = [Step::No; 256];
        for range in line.split(',') {
            let (range, ends) = match range.trim().split_once('.') {
                Some((range, _)) => (range, true),
                None => (range.trim(), false),
            };
            let (range, on) = match range.split_once(':') {
                Some((range, on)) if !ends => (range, Step::On(hex(on))),
                _ => (range, Step::End),
            };
            let (from, to) = range.split_once('-').unwrap_or((range, range));
            row[hex(from)..=hex(to)].fill(on);
        }
        row
    };
    states.iter().map(|line| state(line)).collect()
}

/// The end of the character that starts at `data[i]` by the states `steps`, and whether
/// it is whole: it is not when a byte in it cannot be where it is, and then it is its
/// first byte and the bytes after it that can start no character, up to that byte.
/// `None` when the data ends before the character does.
fn character(steps: &[[Step; 256]], data: &[u8], i: usize) -> Option<(usize, bool)> {
    let (mut state, mut end) = (0, i);
    let whole = loop {
        let &b = data.get(end)?;
        end += 1;
        match steps[state][usize::from(b)] {
            Step::On(next) => state = next,
            step => break step == Step::End,
        }
    };
    if !whole {
        let met = end;
        end = i + 1;
        while end < met && steps[0][usize::from(data[end])] == Step::No {
            end += 1;
        }
    }
    Some((end, whole))
}

/// The bytes of one whole character by the table of `encoding_rs` and the single bytes
/// of `Multi::byte`, without the characters of `Multi::fixes`: `false`, and nothing
/// written, when they are no character there.
fn by_table(of: Multi, bytes: &[u8], out: &mut String) -> bool {
    match (of.byte(bytes[0]), bytes.len()) {
        (Byte::Is(c), 1) => out.push(c),
        (Byte::Table, 1) if bytes[0].is_ascii() => out.push(bytes[0].into()),
        (Byte::Nothing, 1) => return false,
        _ => {
            let before = out.len();
            out.reserve(4 * bytes.len() + 4);
            let mut decoder = of.table().new_decoder_without_bom_handling();
            let (result, _) = decoder.decode_to_string_without_replacement(bytes, out, true);
            if result != encoding_rs::DecoderResult::InputEmpty || out.len() == before {
                out.truncate(before);
                return false;
            }
        }
    }
    true
}

/// A charset of more bytes than one as ICU's table converter reads it (ucnvmbcs.cpp). The
/// bytes of a character that ends are looked up, in the characters ICU has otherwise
/// (`Multi::fixes`) and then in the table of `encoding_rs`; what has no character there is
/// U+FFFD, or U+001A for one byte in a charset that has that substitute. A character that
/// meets a byte it cannot go on with is its first byte and the bytes after it that can
/// start no character, one U+FFFD (or U+001A) for all of them, and the next byte is read
/// as a start. A character the end of the data cuts is left out.
fn multi(of: Multi, data: &[u8]) -> String {
    let steps = steps(of.states());
    let fixes = of.fixes();
    let mut out = String::with_capacity(data.len());
    let mut i = 0;
    while let Some((end, whole)) = character(&steps, data, i) {
        let bytes = &data[i..end];
        if let Some(c) = fixes.of(bytes).filter(|_| whole) {
            out.push(c);
        } else if !(whole && by_table(of, bytes, &mut out)) {
            out.push(if bytes.len() == 1 && of.one_byte_is_sub() {
                '\u{1A}'
            } else {
                '\u{FFFD}'
            });
        }
        i = end;
    }
    out
}

/// The text a decoder made: UTF-16 units when a surrogate in it has no pair, which a string
/// of JavaScript holds and a `String` does not.
pub(crate) enum Text {
    Whole(String),
    Units(Vec<u16>),
}

impl Text {
    fn of(units: Vec<u16>) -> Text {
        match String::from_utf16(&units) {
            Ok(whole) => Text::Whole(whole),
            Err(_) => Text::Units(units),
        }
    }

    /// The text with U+FFFD for a surrogate without its pair.
    pub(crate) fn lossy(self) -> String {
        match self {
            Text::Whole(whole) => whole,
            Text::Units(units) => String::from_utf16_lossy(&units),
        }
    }
}

/// `data` as `charset` reads it. Qt's own UTF-16 and UTF-32 decoders drop a mark at the
/// start. What is not written here reads as UTF-8: `find` never gives it.
pub(crate) fn text(charset: Charset, data: &[u8]) -> Text {
    let marked = |le: &[u8], be: &[u8]| {
        if data.starts_with(be) {
            (true, &data[be.len()..])
        } else {
            (false, data.strip_prefix(le).unwrap_or(data))
        }
    };
    Text::Whole(match charset {
        Charset::Utf8 | Charset::NotWritten => utf8(data),
        Charset::Utf16 => {
            let (big, data) = marked(&[0xFF, 0xFE], &[0xFE, 0xFF]);
            return Text::of(utf16(data, big));
        }
        Charset::Utf16Le => {
            return Text::of(utf16(
                data.strip_prefix(&[0xFF, 0xFE]).unwrap_or(data),
                false,
            ));
        }
        Charset::Utf16Be => {
            return Text::of(utf16(
                data.strip_prefix(&[0xFE, 0xFF]).unwrap_or(data),
                true,
            ));
        }
        Charset::Utf32 => {
            let (big, data) = marked(&[0xFF, 0xFE, 0, 0], &[0, 0, 0xFE, 0xFF]);
            utf32(data, big)
        }
        Charset::Utf32Le => utf32(
            data.strip_prefix(&[0xFF, 0xFE, 0, 0]).unwrap_or(data),
            false,
        ),
        Charset::Utf32Be => utf32(data.strip_prefix(&[0, 0, 0xFE, 0xFF]).unwrap_or(data), true),
        Charset::Latin1 => data.iter().map(|b| *b as char).collect(),
        Charset::Ascii => data
            .iter()
            .map(|b| if b.is_ascii() { *b as char } else { '\u{FFFD}' })
            .collect(),
        Charset::IcuUtf8(cesu) => return Text::of(icu_utf8(data, cesu)),
        Charset::IcuUtf16(big) => return Text::of(icu_utf16(data, big)),
        Charset::IcuUtf32(big) => icu_utf32(data, big),
        Charset::Single(table, but) => single(table, but, data),
        Charset::Multi(of) => multi(of, data),
        Charset::Iso2022Jp => return Text::of(crate::xhr_iso2022jp::decode(data)),
    })
}

/// [`text`] for a reader that takes no surrogate without its pair.
pub(crate) fn decode(charset: Charset, data: &[u8]) -> String {
    text(charset, data).lossy()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QUtf8's decoder: each byte that cannot be there is U+FFFD, a character cut by the
    /// end is dropped, a mark at the start too.
    #[test]
    fn utf8_is_read_as_qt_reads_it() {
        assert_eq!(utf8(b"\xef\xbb\xbfh\xc3\xa9"), "hé");
        assert_eq!(utf8(b"a\xe9b\xc3"), "a\u{FFFD}b");
        assert_eq!(utf8(b"\xc0\xafa"), "\u{FFFD}\u{FFFD}a");
        assert_eq!(utf8(b"\xed\xa0\x80b"), "\u{FFFD}\u{FFFD}\u{FFFD}b");
        assert_eq!(
            utf8(b"\xf4\x90\x80\x80c"),
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}c"
        );
        assert_eq!(
            utf8(b"\xe2\x82e\xf0\x9f\x98\x80\xe2\x82"),
            "\u{FFFD}\u{FFFD}e😀"
        );
    }

    /// A charset name is found without regard to case and punctuation; ISO-8859-1 is itself,
    /// not windows-1252.
    #[test]
    fn charsets_are_found_by_name() {
        assert_eq!(charset(b"\"ISO_8859-1\""), Some(Charset::Latin1));
        assert_eq!(decode(Charset::Latin1, b"\x80\xe9"), "\u{80}é");
        assert_eq!(charset(b"UTF8"), Some(Charset::Utf8));
        let decoded = |name: &[u8], data: &[u8]| charset(name).map(|c| decode(c, data));
        assert_eq!(decoded(b"Windows-1251", b"\xcf"), Some("П".into()));
        assert_eq!(decoded(b"koi8_r", b"\xf0"), Some("П".into()));
        assert_eq!(decoded(b"iso8859-5", b"\xbf"), Some("П".into()));
        assert_eq!(charset(b"foo-bar"), None);
        assert_eq!(charset(b"ISO-8859-1; x=y"), None);
    }

    /// The order of QQmlXMLHttpRequest::findTextDecoder.
    #[test]
    fn the_decoder_is_looked_for_in_qts_order() {
        let xml = b"<?xml version=\"1.0\" encoding='koi8-r'?><a/>";
        assert_eq!(
            find(b"", true, b"text/xml", xml).0,
            charset(b"koi8-r").unwrap()
        );
        assert_eq!(find(b"", false, b"text/plain", xml).0, Charset::Utf8);
        assert_eq!(find(b"utf-16", true, b"text/xml", xml).0, Charset::Utf16);
        let html = b"<META http-equiv=x content=\"text/html; charset=KOI8-R\">";
        assert_eq!(
            find(b"", false, b"text/html", html).0,
            charset(b"koi8-r").unwrap()
        );
        assert_eq!(find(b"", false, b"text/plain", html).0, Charset::Utf8);
        assert_eq!(
            find(b"", false, b"text/plain", b"\xff\xfeh\x00").0,
            Charset::Utf16Le
        );
        assert_eq!(
            find(b"", false, b"", b"\xff\xfe\x00\x00h\x00\x00\x00").0,
            Charset::Utf32Le
        );
    }

    /// ICU compares names by their letters and digits, a number without the zeros it starts
    /// with; Qt's own names are compared first, and only `-` and `_` are passed over there.
    #[test]
    fn names_are_compared_as_qt_and_icu_compare_them() {
        assert_eq!(icu_key(b"ISO_8859-01:1987"), "iso885911987");
        assert_eq!(icu_key(b"ibm-01208"), "ibm1208");
        assert_eq!(icu_key(b"cp 1-00"), "cp10");
        assert_eq!(icu_key(b"x100"), "x100");
        assert_eq!(charset(b"UTF_16"), Some(Charset::Utf16));
        assert_eq!(charset(b"utf 16"), Some(Charset::IcuUtf16(None)));
        assert_eq!(charset(b"ucs-2"), Some(Charset::IcuUtf16(None)));
        assert_eq!(charset(b"Locale"), Some(Charset::Utf8));
        assert_eq!(charset(b"latin-1"), Some(Charset::Latin1));
        assert_eq!(charset(b"l1"), Some(Charset::Latin1));
        assert_eq!(charset(b"iso-8859-16"), None);
    }

    /// `UTF-8` is Qt's own decoder and the other names of ICU's converter for UTF-8 are
    /// ICU's. The two read a mark at the start differently (Qt's drops it), and a character
    /// that stops short: Qt's has U+FFFD for each of its bytes, ICU's one for all of them.
    #[test]
    fn utf8_by_its_own_name_is_read_by_qts_decoder_and_not_by_icus() {
        for own in [&b"UTF-8"[..], b"utf8", b"UTF_8"] {
            assert_eq!(charset(own), Some(Charset::Utf8));
        }
        for icus in [&b"ibm-1208"[..], b"cp1208", b"unicode-1-1-utf-8", b"utf 8"] {
            assert_eq!(charset(icus), Some(Charset::IcuUtf8(false)));
        }
        assert_eq!(charset(b"CESU-8"), Some(Charset::IcuUtf8(true)));
        let bad = b"\xef\xbb\xbf\xe2\x82e\xed\xa0\x80\xf4\x90\x80\x80c";
        assert_eq!(
            decode(Charset::Utf8, bad),
            "\u{FFFD}\u{FFFD}e\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}c"
        );
        assert_eq!(
            decode(Charset::IcuUtf8(false), bad),
            "\u{FEFF}\u{FFFD}e\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}c"
        );
    }

    /// `ibm-5478` and the other names of its converter are a charset of two bytes for every
    /// character, ASCII too, that is not written here: they are not read as GBK. A reply
    /// that names it is read as one that names nothing, and `find` hands the name on.
    #[test]
    fn ibm_5478_is_not_gbk() {
        let names: Vec<&str> = ICU_NAMES
            .lines()
            .filter_map(|l| l.strip_prefix("ibm-5478_P100-1995\t"))
            .collect();
        assert!(names.contains(&"ibm-5478") && names.contains(&"chinese"));
        for name in names {
            assert_eq!(
                charset(name.as_bytes()),
                Some(Charset::NotWritten),
                "{name}"
            );
        }
        assert_eq!(charset(b"GBK"), Some(Charset::Multi(Multi::Gbk)));
        assert_eq!(charset(b"ISO-2022-KR"), Some(Charset::NotWritten));
        assert_eq!(
            find(b"ibm-5478", false, b"text/plain", b"\xc4\xe3"),
            (Charset::Utf8, Some(b"ibm-5478".to_vec()))
        );
        assert_eq!(
            find(b"ibm-5478", false, b"text/plain", b"\xff\xfeh\x00"),
            (Charset::Utf16Le, Some(b"ibm-5478".to_vec()))
        );
        assert_eq!(
            find(b"x-nobody-knows", false, b"", b"a"),
            (Charset::Utf8, None)
        );
    }

    /// A character the end of the reply cuts is left out, without a substitute for it.
    #[test]
    fn a_character_the_end_cuts_is_left_out() {
        let cut: [(&str, &[u8], &str); 12] = [
            ("Shift_JIS", b"\x82\xA0\x82", "あ"),
            ("EUC-JP", b"\xA4\xA2\x8F\xB0", "あ"),
            ("Big5", b"\xA4\x40\xA4", "一"),
            ("Big5-HKSCS", b"\xA4\x40\xA4", "一"),
            ("GBK", b"\xC4\xE3\xBA", "你"),
            ("GB2312", b"\xC4\xE3\xBA", "你"),
            ("gb18030", b"\xC4\xE3\x81\x30\x81", "你"),
            ("EUC-KR", b"\xC7\xD1\xB1", "한"),
            ("windows-949", b"\xC7\xD1\xB1", "한"),
            ("ISO-2022-JP", b"\x1B$B\x24\x22\x24", "あ"),
            ("ibm-1208", b"\xE3\x81\x82\xE3\x81", "あ"),
            ("ucs-2", b"\x30\x42\xD8\x3D", "あ"),
        ];
        for (name, bytes, text) in cut {
            assert_eq!(
                decode(charset(name.as_bytes()).unwrap(), bytes),
                text,
                "{name}"
            );
        }
    }

    /// What ICU itself reads each of `inputs` as by the converter `name`, a whole reply
    /// each: `crates/qml/tests/qt_icu.py` asks the ICU that Qt 6.12 loads. `None` where
    /// that Qt is not installed.
    fn icu(name: &str, inputs: &[Vec<u8>]) -> Option<Vec<Vec<u16>>> {
        const PYTHON: &str = "/opt/qv4-venv/bin/python";
        if !std::path::Path::new(PYTHON).exists() {
            crate::testenv::unavailable(format_args!("{PYTHON} (PySide6) is not installed"));
            return None;
        }
        let records: Vec<u8> = inputs
            .iter()
            .flat_map(|i| [&[u8::try_from(i.len()).unwrap()], &i[..]].concat())
            .collect();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), records).unwrap();
        let out = std::process::Command::new(PYTHON)
            .args(["-c", include_str!("../tests/qt_icu.py"), "decode", name])
            .arg(file.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut rest = &out.stdout[..];
        let mut read = Vec::with_capacity(inputs.len());
        while let Some((&count, units)) = rest.split_first() {
            let (units, after) = units.split_at(2 * usize::from(count));
            read.push(
                units
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&u| u16::from_le_bytes(u))
                    .collect(),
            );
            rest = after;
        }
        assert_eq!(read.len(), inputs.len(), "{name}");
        Some(read)
    }

    /// Every sequence of one byte and of two.
    fn short() -> Vec<Vec<u8>> {
        let one = (0..=255).map(|a| vec![a]);
        one.chain((0..=255).flat_map(|a| (0..=255).map(move |b| vec![a, b])))
            .collect()
    }

    /// `short` after `start`.
    fn after(start: &[u8]) -> Vec<Vec<u8>> {
        short().iter().map(|s| [start, s].concat()).collect()
    }

    /// The sequences a decoder is measured on, each a reply of its own: the short ones, the
    /// longer characters a charset has, and `streams` replies of 3 to 12 bytes that are the
    /// same on every run, a third of their bytes from `often`.
    fn replies(charset: Charset, streams: usize, often: &[u8]) -> Vec<Vec<u8>> {
        let mut all = short();
        match charset {
            Charset::Single(..) | Charset::Latin1 | Charset::Ascii => all.truncate(256),
            Charset::Multi(Multi::EucJp) => all.extend(after(&[0x8F])),
            Charset::Multi(Multi::Gb18030) => {
                for a in 0x81..=0xFE {
                    for b in 0x30..=0x39 {
                        for c in 0x81..=0xFE {
                            all.extend((0x30..=0x39).map(|d| vec![a, b, c, d]));
                        }
                    }
                }
            }
            Charset::IcuUtf8(_) => {
                for a in 0xE0..=0xEF {
                    all.extend(after(&[a]));
                }
                for a in 0xF0..=0xF7 {
                    for b in 0..=255 {
                        for c in [0x7F, 0x80, 0xBF, 0xC0] {
                            all.extend([0x7F, 0x80, 0xBF, 0xC0].map(|d| vec![a, b, c, d]));
                        }
                    }
                }
                for a in 0xA0..=0xBF {
                    for b in [0x80, 0xBF] {
                        for c in 0xA0..=0xBF {
                            all.extend([0x80, 0xBF].map(|d| vec![0xED, a, b, 0xED, c, d]));
                        }
                    }
                }
            }
            _ => {}
        }
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = |below: usize| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize % below
        };
        for _ in 0..streams {
            let len = 3 + next(10);
            all.push(
                (0..len)
                    .map(|_| match next(3) {
                        0 => often[next(often.len())],
                        _ => next(256) as u8,
                    })
                    .collect(),
            );
        }
        all
    }

    /// The bytes the streams of `replies` have often: where the states of the charsets
    /// turn, the marks, the surrogates.
    const OFTEN: [u8; 30] = [
        0x00, 0x1A, 0x30, 0x39, 0x40, 0x7E, 0x7F, 0x80, 0x81, 0x8E, 0x8F, 0xA0, 0xA1, 0xBF, 0xC0,
        0xD8, 0xDB, 0xDC, 0xDF, 0xE0, 0xE4, 0xED, 0xEF, 0xF0, 0xF4, 0xFC, 0xFD, 0xFE, 0xFF, 0xBB,
    ];

    /// How the replies of `replies` that are read otherwise than ICU reads them differ, by
    /// the first unit that differs: ICU has a character and the text here another (or the
    /// texts differ in length); ICU has a character and the text here a substitute (U+FFFD
    /// or U+001A); the text here has a character and ICU a substitute. A stream that
    /// differs counts only when no short sequence that differs is in it.
    #[derive(Debug, PartialEq, Default)]
    struct Differs {
        other_character: usize,
        only_icu_reads: usize,
        only_we_read: usize,
        streams: usize,
    }

    fn measured(name: &str, charset: Charset, streams: usize) -> Option<(usize, Differs)> {
        let replies = replies(charset, streams, &OFTEN);
        let icu = icu(name, &replies)?;
        let mut differs = Differs::default();
        let mut known = std::collections::HashSet::new();
        let listed = replies.len() - streams;
        // What tools/gen/charset/gen.py makes xhr_charset_data.rs from: the characters ICU reads
        // otherwise than the table of `encoding_rs`, the bytes and then ICU's units in hex.
        let of = match charset {
            Charset::Multi(of) => std::env::var_os("XHR_CHARSET_DUMP").map(|dir| (dir, of)),
            _ => None,
        };
        let mut dump = of.as_ref().map(|_| Vec::new());
        for (at, (reply, icu)) in replies.iter().zip(icu).enumerate() {
            let ours: Vec<u16> = match (text(charset, reply), &of) {
                (_, Some((_, of))) if at < listed => {
                    let mut by = String::new();
                    let steps = steps(of.states());
                    match character(&steps, reply, 0) {
                        Some((end, true)) if end == reply.len() => {
                            if !by_table(*of, reply, &mut by) {
                                by.push(if end == 1 && of.one_byte_is_sub() {
                                    '\u{1A}'
                                } else {
                                    '\u{FFFD}'
                                });
                            }
                            by.encode_utf16().collect()
                        }
                        _ => icu.clone(),
                    }
                }
                (Text::Whole(whole), _) => whole.encode_utf16().collect(),
                (Text::Units(units), _) => units,
            };
            if ours == icu {
                continue;
            }
            if at >= listed {
                let has_known = (1..=4).any(|n| reply.windows(n).any(|w| known.contains(w)));
                differs.streams += usize::from(!has_known);
                continue;
            }
            known.insert(reply.clone());
            if let Some(lines) = dump.as_mut() {
                lines.push((reply.clone(), icu.clone()));
            }
            let first = ours.iter().zip(&icu).position(|(a, b)| a != b);
            let substitute =
                |units: &[u16]| first.is_some_and(|k| matches!(units[k], 0xFFFD | 0x1A));
            if substitute(&icu) {
                differs.only_we_read += 1;
            } else if substitute(&ours) {
                differs.only_icu_reads += 1;
            } else {
                differs.other_character += 1;
            }
        }
        if let (Some((dir, of)), Some(lines)) = (of, dump) {
            let hex = |units: &[u16]| units.iter().map(|u| format!("{u:04x}")).collect::<String>();
            let line = |(bytes, icu): &(Vec<u8>, Vec<u16>)| {
                let bytes: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                format!("{bytes}\t{}\n", hex(icu))
            };
            let file = std::path::Path::new(&dir).join(format!("{of:?}.tsv"));
            std::fs::write(file, lines.iter().map(line).collect::<String>()).unwrap();
        }
        Some((listed, differs))
    }

    /// Every converter of ICU that is written here, with a name Qt gives to ICU for it.
    fn written() -> Vec<(&'static str, Charset)> {
        let mut seen = std::collections::HashSet::new();
        ICU_NAMES
            .lines()
            .map(|l| l.split_once('\t').unwrap())
            .filter(|(of, name)| own(name.as_bytes()).is_none() && seen.insert(*of))
            .map(|(of, name)| (name, converter(of)))
            .filter(|(_, c)| !matches!(c, Charset::NotWritten | Charset::Iso2022Jp))
            .collect()
    }

    /// Each converter of ICU written here reads every reply measured as ICU reads it:
    /// every sequence of one byte and of two, the longer ones of its charset, and 60 000
    /// streams. In the charsets of more bytes than one that is every character of the
    /// table, a character the end cuts, a byte that cannot be where it is and a character
    /// the table has not.
    #[test]
    fn the_converters_of_icu_read_as_icu() {
        let results: Vec<_> = std::thread::scope(|s| {
            let runs: Vec<_> = written()
                .into_iter()
                .map(|(name, c)| (name, c, s.spawn(move || measured(name, c, 60_000))))
                .collect();
            runs.into_iter()
                .map(|(name, c, run)| (name, c, run.join().unwrap()))
                .collect()
        });
        let mut wrong = String::new();
        for (name, charset, result) in results {
            let Some((listed, differs)) = result else {
                return;
            };
            let want_listed = match charset {
                Charset::Multi(Multi::EucJp) => 131_584,
                Charset::Multi(Multi::Gb18030) => 1_653_392,
                Charset::Multi(_) => 65_792,
                _ => listed,
            };
            if (listed, &differs) != (want_listed, &Differs::default()) {
                wrong += &format!("{name} ({charset:?}): {listed} sequences, {differs:?}\n");
            }
        }
        assert!(wrong.is_empty(), "\n{wrong}");
    }

    /// The characters made from ICU (xhr_charset_data.rs) go up by their bytes, each is
    /// one whole character by the states of its charset, and none is what the table of
    /// `encoding_rs` has anyway.
    #[test]
    fn the_characters_taken_from_icu_are_in_order_and_none_is_the_tables() {
        use Multi::*;
        let all = [
            ShiftJis, EucJp, Big5, Big5Hkscs, Gbk, Gb2312, Gb18030, EucKr, Windows949,
        ];
        let mut count = 0;
        for of in all {
            let fixes = of.fixes();
            let two = fixes.two.iter().map(|&(k, c)| (u32::from(k), u32::from(c)));
            let more = fixes.more.iter().map(|&(k, c)| (k, u32::from(c)));
            assert!(fixes.two.is_sorted_by_key(|fix| fix.0), "{of:?}");
            assert!(fixes.more.is_sorted_by_key(|fix| fix.0), "{of:?}");
            let steps = steps(of.states());
            for (key, c) in two.chain(more) {
                let bytes = key.to_be_bytes();
                let bytes = &bytes[key.leading_zeros() as usize / 8..];
                let c = char::from_u32(c).unwrap();
                assert_eq!(
                    character(&steps, bytes, 0),
                    Some((bytes.len(), true)),
                    "{of:?} {key:x}"
                );
                assert_eq!(fixes.of(bytes), Some(c), "{of:?} {key:x}");
                let mut table = String::new();
                by_table(of, bytes, &mut table);
                assert_ne!(table, c.to_string(), "{of:?} {key:x}");
                count += 1;
            }
        }
        assert_eq!(count, 8938);
    }
}
