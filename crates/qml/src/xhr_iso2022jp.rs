//! ISO-2022-JP as Qt 6.12 reads it in an XMLHttpRequest reply: ICU 73's converter
//! `ISO_2022,locale=ja,version=0` (ucnv2022.cpp), given the whole reply at once with nothing
//! flushed, each error replaced by U+FFFD.

/// The character set that bytes are read in.
#[derive(Clone, Copy)]
enum Set {
    Ascii,
    /// JIS X 0201 Roman: ASCII with a yen sign and an overline.
    Roman,
    /// The half-width katakana of JIS X 0201 in seven bits.
    Kana,
    /// JIS X 0208: two bytes a character.
    Jis208,
}

/// What ICU's table of escape sequences has for the bytes from an ESC on.
enum Escape {
    /// The start of longer sequences.
    More,
    To(Set),
    /// A whole sequence that this version of the converter does not read.
    Unsupported,
    /// No sequence starts so.
    Illegal,
}

fn escape(bytes: &[u8]) -> Escape {
    match bytes {
        [0x1B]
        | [0x1B, b'(' | b'$' | b'%' | b'.' | b'&']
        | [0x1B, b'$', b'(' | b')' | b'*' | b'+']
        | [0x1B, b'%', b'/'] => Escape::More,
        [0x1B, b'(', b'B'] => Escape::To(Set::Ascii),
        [0x1B, b'(', b'H' | b'J'] => Escape::To(Set::Roman),
        [0x1B, b'(', b'I'] => Escape::To(Set::Kana),
        [0x1B, b'$', b'@' | b'B'] | [0x1B, b'&', b'@'] => Escape::To(Set::Jis208),
        [0x1B, b'O']
        | [0x1B, b'(', b'@' | b'A' | b'C'..=b'G' | b'K' | b'R']
        | [0x1B, b'$', b'A']
        | [0x1B, b'%', b'B']
        | [0x1B, b'.', b'A' | b'F']
        | [0x1B, b'$', b'(', b'@'..=b'E' | b'G'..=b'M']
        | [0x1B, b'$', b')', b'A' | b'C' | b'E' | b'G']
        | [0x1B, b'$', b'*', b'H']
        | [0x1B, b'$', b'+', b'I'..=b'M']
        | [0x1B, b'%', b'/', b'@' | b'A' | b'C'..=b'F'] => Escape::Unsupported,
        // Among these ESC N, a shift to a set that this version never has.
        _ => Escape::Illegal,
    }
}

/// The UTF-16 units of the reply `data`. A character or an escape sequence that the end of
/// the reply cuts gives nothing.
pub(crate) fn decode(data: &[u8]) -> Vec<u16> {
    const BAD: u16 = 0xFFFD;
    let mut out = Vec::with_capacity(data.len());
    let mut jis = encoding_rs::EUC_JP.new_decoder_without_bom_handling();
    let mut set = Set::Ascii;
    // An escape sequence has set the character set and nothing has been read since.
    let mut empty = false;
    let mut at = 0;
    while at < data.len() {
        let b = data[at];
        at += 1;
        if b == 0x1B {
            let mut end = at;
            let found = loop {
                match escape(&data[at - 1..end]) {
                    Escape::More if end == data.len() => return out,
                    Escape::More => end += 1,
                    found => break found,
                }
            };
            match found {
                Escape::To(to) => {
                    set = to;
                    at = end;
                    // The second of two escape sequences with nothing between them is an
                    // error, and still sets the character set.
                    if empty {
                        out.push(BAD);
                    }
                    empty = !empty;
                }
                Escape::Unsupported => {
                    at = end;
                    out.push(BAD);
                    empty = false;
                }
                // The ESC alone is the error: the bytes after it are read again.
                _ => {
                    out.push(BAD);
                    empty = false;
                }
            }
            continue;
        }
        empty = false;
        // The end of a line ends katakana and JIS X 0208.
        if matches!(b, b'\r' | b'\n') && matches!(set, Set::Kana | Set::Jis208) {
            set = Set::Ascii;
        }
        out.push(match (set, b) {
            // SO and SI.
            (_, 0x0E | 0x0F) => BAD,
            (Set::Roman, b'\\') => 0xA5,
            (Set::Roman, b'~') => 0x203E,
            (Set::Ascii | Set::Roman, 0..0x80) => b.into(),
            (Set::Kana, 0x21..0x60) => 0xFF61 + u16::from(b - 0x21),
            (Set::Jis208, _) => {
                let Some(&second) = data.get(at) else {
                    return out;
                };
                let printable = |b| (0x21..0x7F).contains(&b);
                if printable(b) && printable(second) {
                    at += 1;
                    // U+FFFD where the table has no character.
                    let mut unit = [0; 4];
                    let _ = jis.decode_to_utf16(&[b | 0x80, second | 0x80], &mut unit, false);
                    unit[0]
                } else {
                    // A second byte that can start a character or an escape sequence is not
                    // part of the error.
                    if !(printable(second) || matches!(second, 0x0E | 0x0F | 0x1B)) {
                        at += 1;
                    }
                    BAD
                }
            }
            _ => BAD,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::decode;

    const PYTHON: &str = "/opt/qv4-venv/bin/python";

    /// What the ICU of Qt 6.12 reads each of `streams` as (tests/qt_icu.py), or nothing
    /// where that Qt is not installed.
    fn icu(streams: &[Vec<u8>]) -> Option<Vec<Vec<u16>>> {
        if !std::path::Path::new(PYTHON).exists() {
            crate::testenv::unavailable(format_args!("{PYTHON} (PySide6) is not installed"));
            return None;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("streams");
        let mut records = Vec::new();
        for s in streams {
            records.push(u8::try_from(s.len()).unwrap());
            records.extend_from_slice(s);
        }
        std::fs::write(&path, records).unwrap();
        let out = std::process::Command::new(PYTHON)
            .args([
                "-c",
                include_str!("../tests/qt_icu.py"),
                "decode",
                "ISO-2022-JP",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut read = Vec::with_capacity(streams.len());
        let mut rest = &out.stdout[..];
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
        assert_eq!(read.len(), streams.len());
        Some(read)
    }

    /// Fails with the count of `streams` that `decode` and ICU read differently, and the
    /// first of them.
    fn read_as_icu_reads(streams: &[Vec<u8>]) {
        let Some(icu) = icu(streams) else { return };
        let differ: Vec<String> = streams
            .iter()
            .zip(&icu)
            .filter_map(|(s, icu)| {
                let here = decode(s);
                (here != *icu).then(|| format!("{s:02x?}: here {here:04x?}, ICU {icu:04x?}"))
            })
            .collect();
        assert!(
            differ.is_empty(),
            "{} of {} streams are read differently; the first:\n{}",
            differ.len(),
            streams.len(),
            differ[..differ.len().min(20)].join("\n")
        );
    }

    /// Every sequence of one and of two bytes, at the start of a reply and after each of
    /// six escape sequences.
    #[test]
    fn short_sequences_are_read_as_icu_reads_them() {
        let starts: [&[u8]; 7] = [
            b"", b"\x1b(B", b"\x1b(J", b"\x1b(I", b"\x1b$@", b"\x1b$B", b"\x1b$(D",
        ];
        let mut streams = Vec::new();
        for start in starts {
            for a in 0..=255 {
                streams.push([start, &[a]].concat());
                for b in 0..=255 {
                    streams.push([start, &[a, b]].concat());
                }
            }
        }
        assert_eq!(streams.len(), 460_544);
        read_as_icu_reads(&streams);
    }

    /// ESC and every one, two and three of the bytes that ICU's escape sequences are made of
    /// and one that is in none, then three bytes that each character set reads differently.
    #[test]
    fn escape_sequences_are_read_as_icu_reads_them() {
        const BYTES: &[u8] = b"\x1b$%&()*+-./@ABCDEFGHIJKLMNORZa";
        let mut streams = Vec::new();
        let mut last = vec![vec![0x1B]];
        for _ in 0..3 {
            last = last
                .iter()
                .flat_map(|s| BYTES.iter().map(move |&b| [&s[..], &[b]].concat()))
                .collect();
            streams.extend(last.iter().map(|s| [&s[..], b"\\!!"].concat()));
        }
        assert_eq!(streams.len(), 27_930);
        read_as_icu_reads(&streams);
    }

    /// Streams of 3 to 12 bytes: a quarter of their parts are whole escape sequences, a
    /// half are bytes that escapes are made of or that end a state, a quarter are any byte.
    #[test]
    fn random_streams_are_read_as_icu_reads_them() {
        const ESCAPES: [&[u8]; 16] = [
            b"\x1b(B", b"\x1b(J", b"\x1b(I", b"\x1b$@", b"\x1b$B", b"\x1b$(D", b"\x1b(H",
            b"\x1b&@", b"\x1bN", b"\x1bO", b"\x1b.A", b"\x1b$A", b"\x1b$(C", b"\x1b$)A",
            b"\x1b%/@", b"\x1b%B",
        ];
        const BYTES: &[u8] = b"\x1b$()BJI@D\x0e\x0f\r\n\x21\x7e\x7f\x80";
        let mut seed = 0x2022_u64;
        let mut below = move |n: usize| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize % n
        };
        let streams: Vec<Vec<u8>> = (0..200_000)
            .map(|_| {
                let len = 3 + below(10);
                let mut s = Vec::new();
                while s.len() < len {
                    match below(4) {
                        0 => s.extend_from_slice(ESCAPES[below(ESCAPES.len())]),
                        1 => s.push(below(256) as u8),
                        _ => s.push(BYTES[below(BYTES.len())]),
                    }
                }
                s.truncate(len);
                s
            })
            .collect();
        read_as_icu_reads(&streams);
    }
}
