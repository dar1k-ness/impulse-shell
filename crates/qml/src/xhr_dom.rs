//! The document `XMLHttpRequest.responseXML` gives: QQmlXMLHttpRequest's DOM (Document::load
//! over QXmlStreamReader, and the Node, Element, Attr, Text and Document objects).

use crate::eval::{Callable, Interp, NativeFn, R, define_accessor, fn_value, new_obj, set_hidden};
use crate::value::{Obj, Value};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Element = 1,
    Attr = 2,
    Text = 3,
    Cdata = 4,
    Document = 9,
}

struct Node {
    kind: Kind,
    /// An element's or an attribute's name without its prefix.
    name: String,
    namespace: String,
    /// An attribute's value, a text's characters.
    data: String,
    /// An attribute's is its element.
    parent: Option<usize>,
    children: Vec<usize>,
    attributes: Vec<usize>,
}

pub(crate) struct Doc {
    nodes: Vec<Node>,
    root: usize,
    version: String,
    encoding: String,
    standalone: bool,
}

impl Doc {
    fn empty() -> Doc {
        Doc {
            nodes: vec![],
            root: 0,
            version: String::new(),
            encoding: String::new(),
            standalone: false,
        }
    }
}

const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// What QXmlStreamReader reads of `data` as text. It starts with the encoding the first
/// bytes show (QStringConverter::encodingForData with `<` expected: a byte order mark, or
/// `<` as one unit of UTF-16 or UTF-32), UTF-8 without one, and reads the XML declaration;
/// the last `encoding` there decodes all of `data` again. `None` when the bytes are not in
/// the encoding that is left. Line ends stay as sent: the tokenizer reads them.
fn text(data: &[u8]) -> Option<String> {
    use crate::xhr_text::{Charset, charset, decode};
    let shown = match data {
        [0, 0, 0xFE, 0xFF, ..] | [0, 0, 0, b'<', ..] => Some(Charset::Utf32Be),
        [0xFF, 0xFE, 0, 0, ..] | [b'<', 0, 0, 0, ..] => Some(Charset::Utf32Le),
        [0xEF, 0xBB, 0xBF, ..] => None,
        [0xFE, 0xFF, ..] | [0, b'<', ..] => Some(Charset::Utf16Be),
        [0xFF, 0xFE, ..] | [b'<', 0, ..] => Some(Charset::Utf16Le),
        _ => None,
    };
    let unmarked = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
    let first = match shown {
        Some(c) => decode(c, data).into(),
        None => String::from_utf8_lossy(unmarked),
    };
    // A converter that is not written here leaves the text as it was read so far.
    let named = match declared_encoding(&first) {
        Some(name) => Some(charset(name.as_bytes())?).filter(|c| *c != Charset::NotWritten),
        None => None,
    };
    match (named, shown) {
        (Some(Charset::Utf8), _) | (None, None) => {
            Some(std::str::from_utf8(unmarked).ok()?.to_string())
        }
        // Its decoder reports a byte that is no ASCII.
        (Some(Charset::Ascii), _) => data.is_ascii().then(|| decode(Charset::Ascii, data)),
        (Some(c), _) => Some(decode(c, data)),
        (None, Some(_)) => Some(first.into_owned()),
    }
}

/// The `encoding` of the XML declaration `text` starts with, if it has one that reads.
fn declared_encoding(text: &str) -> Option<String> {
    let end = text.strip_prefix("<?xml")?.find("?>")? + 7;
    let mut doc = Doc::empty();
    Reader::new(&text[..end], false).declaration(&mut doc)?;
    (!doc.encoding.is_empty()).then_some(doc.encoding)
}

bits::bit_flags! {
/// Marks a character QXmlStreamReader put back as text whatever it is (putStringLiteral's
/// LETTER token): what a character reference or a predefined entity stands for.
const TEXT: u32 = 1 << 31;
/// Where the text of a referenced entity ends: the ENTITY_DONE token.
const DONE: u32 = 1 << 30;
/// The end of the input.
const END: u32 = 1 << 29;
}

/// The character of `c` without its mark; `'\0'` for [`DONE`] and [`END`].
fn letter_of(c: u32) -> char {
    char::from_u32(c & 0x1F_FFFF).unwrap_or('\0')
}

/// `c` as the character it is when it has no mark.
fn plain(c: u32) -> Option<char> {
    char::from_u32(c)
}

fn is_space(c: u32) -> bool {
    matches!(plain(c), Some(' ' | '\t' | '\n' | '\r'))
}

/// Whether `c` is what the tokenizer calls a LETTER, which is what a name starts with: a
/// marked character, or one that is no white space, digit, control or punctuation of XML.
fn is_letter(c: u32) -> bool {
    match plain(c) {
        Some(c) => {
            c > ' '
                && !c.is_ascii_digit()
                && !"&#'\"<>[]()|=%/:;,-+*.?!\u{fffe}\u{ffff}".contains(c)
        }
        None => c & TEXT != 0,
    }
}

/// Whether the tokenizer has a token for `c` as it is written: no control character.
fn is_token(c: u32) -> bool {
    match plain(c) {
        Some(c) => {
            (c >= ' ' || matches!(c, '\t' | '\n' | '\r')) && !matches!(c, '\u{fffe}' | '\u{ffff}')
        }
        None => c & TEXT != 0,
    }
}

/// The letters of XML 1.0, fourth edition (BaseChar and Ideographic), as QXmlUtils has
/// them: the first and the last of each range.
const LETTERS: [u16; 408] = [
    0x0041, 0x005A, 0x0061, 0x007A, 0x00C0, 0x00D6, 0x00D8, 0x00F6, 0x00F8, 0x0131, 0x0134, 0x013E,
    0x0141, 0x0148, 0x014A, 0x017E, 0x0180, 0x01C3, 0x01CD, 0x01F0, 0x01F4, 0x01F5, 0x01FA, 0x0217,
    0x0250, 0x02A8, 0x02BB, 0x02C1, 0x0386, 0x0386, 0x0388, 0x038A, 0x038C, 0x038C, 0x038E, 0x03A1,
    0x03A3, 0x03CE, 0x03D0, 0x03D6, 0x03DA, 0x03DA, 0x03DC, 0x03DC, 0x03DE, 0x03DE, 0x03E0, 0x03E0,
    0x03E2, 0x03F3, 0x0401, 0x040C, 0x040E, 0x044F, 0x0451, 0x045C, 0x045E, 0x0481, 0x0490, 0x04C4,
    0x04C7, 0x04C8, 0x04CB, 0x04CC, 0x04D0, 0x04EB, 0x04EE, 0x04F5, 0x04F8, 0x04F9, 0x0531, 0x0556,
    0x0559, 0x0559, 0x0561, 0x0586, 0x05D0, 0x05EA, 0x05F0, 0x05F2, 0x0621, 0x063A, 0x0641, 0x064A,
    0x0671, 0x06B7, 0x06BA, 0x06BE, 0x06C0, 0x06CE, 0x06D0, 0x06D3, 0x06D5, 0x06D5, 0x06E5, 0x06E6,
    0x0905, 0x0939, 0x093D, 0x093D, 0x0958, 0x0961, 0x0985, 0x098C, 0x098F, 0x0990, 0x0993, 0x09A8,
    0x09AA, 0x09B0, 0x09B2, 0x09B2, 0x09B6, 0x09B9, 0x09DC, 0x09DD, 0x09DF, 0x09E1, 0x09F0, 0x09F1,
    0x0A05, 0x0A0A, 0x0A0F, 0x0A10, 0x0A13, 0x0A28, 0x0A2A, 0x0A30, 0x0A32, 0x0A33, 0x0A35, 0x0A36,
    0x0A38, 0x0A39, 0x0A59, 0x0A5C, 0x0A5E, 0x0A5E, 0x0A72, 0x0A74, 0x0A85, 0x0A8B, 0x0A8D, 0x0A8D,
    0x0A8F, 0x0A91, 0x0A93, 0x0AA8, 0x0AAA, 0x0AB0, 0x0AB2, 0x0AB3, 0x0AB5, 0x0AB9, 0x0ABD, 0x0ABD,
    0x0AE0, 0x0AE0, 0x0B05, 0x0B0C, 0x0B0F, 0x0B10, 0x0B13, 0x0B28, 0x0B2A, 0x0B30, 0x0B32, 0x0B33,
    0x0B36, 0x0B39, 0x0B3D, 0x0B3D, 0x0B5C, 0x0B5D, 0x0B5F, 0x0B61, 0x0B85, 0x0B8A, 0x0B8E, 0x0B90,
    0x0B92, 0x0B95, 0x0B99, 0x0B9A, 0x0B9C, 0x0B9C, 0x0B9E, 0x0B9F, 0x0BA3, 0x0BA4, 0x0BA8, 0x0BAA,
    0x0BAE, 0x0BB5, 0x0BB7, 0x0BB9, 0x0C05, 0x0C0C, 0x0C0E, 0x0C10, 0x0C12, 0x0C28, 0x0C2A, 0x0C33,
    0x0C35, 0x0C39, 0x0C60, 0x0C61, 0x0C85, 0x0C8C, 0x0C8E, 0x0C90, 0x0C92, 0x0CA8, 0x0CAA, 0x0CB3,
    0x0CB5, 0x0CB9, 0x0CDE, 0x0CDE, 0x0CE0, 0x0CE1, 0x0D05, 0x0D0C, 0x0D0E, 0x0D10, 0x0D12, 0x0D28,
    0x0D2A, 0x0D39, 0x0D60, 0x0D61, 0x0E01, 0x0E2E, 0x0E30, 0x0E30, 0x0E32, 0x0E33, 0x0E40, 0x0E45,
    0x0E81, 0x0E82, 0x0E84, 0x0E84, 0x0E87, 0x0E88, 0x0E8A, 0x0E8A, 0x0E8D, 0x0E8D, 0x0E94, 0x0E97,
    0x0E99, 0x0E9F, 0x0EA1, 0x0EA3, 0x0EA5, 0x0EA5, 0x0EA7, 0x0EA7, 0x0EAA, 0x0EAB, 0x0EAD, 0x0EAE,
    0x0EB0, 0x0EB0, 0x0EB2, 0x0EB3, 0x0EBD, 0x0EBD, 0x0EC0, 0x0EC4, 0x0F40, 0x0F47, 0x0F49, 0x0F69,
    0x10A0, 0x10C5, 0x10D0, 0x10F6, 0x1100, 0x1100, 0x1102, 0x1103, 0x1105, 0x1107, 0x1109, 0x1109,
    0x110B, 0x110C, 0x110E, 0x1112, 0x113C, 0x113C, 0x113E, 0x113E, 0x1140, 0x1140, 0x114C, 0x114C,
    0x114E, 0x114E, 0x1150, 0x1150, 0x1154, 0x1155, 0x1159, 0x1159, 0x115F, 0x1161, 0x1163, 0x1163,
    0x1165, 0x1165, 0x1167, 0x1167, 0x1169, 0x1169, 0x116D, 0x116E, 0x1172, 0x1173, 0x1175, 0x1175,
    0x119E, 0x119E, 0x11A8, 0x11A8, 0x11AB, 0x11AB, 0x11AE, 0x11AF, 0x11B7, 0x11B8, 0x11BA, 0x11BA,
    0x11BC, 0x11C2, 0x11EB, 0x11EB, 0x11F0, 0x11F0, 0x11F9, 0x11F9, 0x1E00, 0x1E9B, 0x1EA0, 0x1EF9,
    0x1F00, 0x1F15, 0x1F18, 0x1F1D, 0x1F20, 0x1F45, 0x1F48, 0x1F4D, 0x1F50, 0x1F57, 0x1F59, 0x1F59,
    0x1F5B, 0x1F5B, 0x1F5D, 0x1F5D, 0x1F5F, 0x1F7D, 0x1F80, 0x1FB4, 0x1FB6, 0x1FBC, 0x1FBE, 0x1FBE,
    0x1FC2, 0x1FC4, 0x1FC6, 0x1FCC, 0x1FD0, 0x1FD3, 0x1FD6, 0x1FDB, 0x1FE0, 0x1FEC, 0x1FF2, 0x1FF4,
    0x1FF6, 0x1FFC, 0x2126, 0x2126, 0x212A, 0x212B, 0x212E, 0x212E, 0x2180, 0x2182, 0x3007, 0x3007,
    0x3021, 0x3029, 0x3041, 0x3094, 0x30A1, 0x30FA, 0x3105, 0x312C, 0x4E00, 0x9FA5, 0xAC00, 0xD7A3,
];

/// Its CombiningChar, Digit and Extender, which a name may hold after its first character.
const NAME_MARKS: [u16; 218] = [
    0x0030, 0x0039, 0x00B7, 0x00B7, 0x02D0, 0x02D1, 0x0300, 0x0345, 0x0360, 0x0361, 0x0387, 0x0387,
    0x0483, 0x0486, 0x0591, 0x05A1, 0x05A3, 0x05B9, 0x05BB, 0x05BD, 0x05BF, 0x05BF, 0x05C1, 0x05C2,
    0x05C4, 0x05C4, 0x0640, 0x0640, 0x064B, 0x0652, 0x0660, 0x0669, 0x0670, 0x0670, 0x06D6, 0x06E4,
    0x06E7, 0x06E8, 0x06EA, 0x06ED, 0x06F0, 0x06F9, 0x0901, 0x0903, 0x093C, 0x093C, 0x093E, 0x094D,
    0x0951, 0x0954, 0x0962, 0x0963, 0x0966, 0x096F, 0x0981, 0x0983, 0x09BC, 0x09BC, 0x09BE, 0x09C4,
    0x09C7, 0x09C8, 0x09CB, 0x09CD, 0x09D7, 0x09D7, 0x09E2, 0x09E3, 0x09E6, 0x09EF, 0x0A02, 0x0A02,
    0x0A3C, 0x0A3C, 0x0A3E, 0x0A42, 0x0A47, 0x0A48, 0x0A4B, 0x0A4D, 0x0A66, 0x0A71, 0x0A81, 0x0A83,
    0x0ABC, 0x0ABC, 0x0ABE, 0x0AC5, 0x0AC7, 0x0AC9, 0x0ACB, 0x0ACD, 0x0AE6, 0x0AEF, 0x0B01, 0x0B03,
    0x0B3C, 0x0B3C, 0x0B3E, 0x0B43, 0x0B47, 0x0B48, 0x0B4B, 0x0B4D, 0x0B56, 0x0B57, 0x0B66, 0x0B6F,
    0x0B82, 0x0B83, 0x0BBE, 0x0BC2, 0x0BC6, 0x0BC8, 0x0BCA, 0x0BCD, 0x0BD7, 0x0BD7, 0x0BE7, 0x0BEF,
    0x0C01, 0x0C03, 0x0C3E, 0x0C44, 0x0C46, 0x0C48, 0x0C4A, 0x0C4D, 0x0C55, 0x0C56, 0x0C66, 0x0C6F,
    0x0C82, 0x0C83, 0x0CBE, 0x0CC4, 0x0CC6, 0x0CC8, 0x0CCA, 0x0CCD, 0x0CD5, 0x0CD6, 0x0CE6, 0x0CEF,
    0x0D02, 0x0D03, 0x0D3E, 0x0D43, 0x0D46, 0x0D48, 0x0D4A, 0x0D4D, 0x0D57, 0x0D57, 0x0D66, 0x0D6F,
    0x0E31, 0x0E31, 0x0E34, 0x0E3A, 0x0E46, 0x0E4E, 0x0E50, 0x0E59, 0x0EB1, 0x0EB1, 0x0EB4, 0x0EB9,
    0x0EBB, 0x0EBC, 0x0EC6, 0x0EC6, 0x0EC8, 0x0ECD, 0x0ED0, 0x0ED9, 0x0F18, 0x0F19, 0x0F20, 0x0F29,
    0x0F35, 0x0F35, 0x0F37, 0x0F37, 0x0F39, 0x0F39, 0x0F3E, 0x0F3F, 0x0F71, 0x0F84, 0x0F86, 0x0F8B,
    0x0F90, 0x0F95, 0x0F97, 0x0F97, 0x0F99, 0x0FAD, 0x0FB1, 0x0FB7, 0x0FB9, 0x0FB9, 0x20D0, 0x20DC,
    0x20E1, 0x20E1, 0x3005, 0x3005, 0x302A, 0x302F, 0x3031, 0x3035, 0x3099, 0x309A, 0x309D, 0x309E,
    0x30FC, 0x30FE,
];

/// What a name token may hold after its first character (fastScanNMTOKEN): the letters,
/// numbers and marks of Unicode as Qt 6.12.0 has them, and `-`, `.`, `:`, `_`. Read off
/// QXmlStreamReader for every character up to U+FFFF; it takes none past that.
const TOKEN_PARTS: [u16; 874] = [
    0x002D, 0x002E, 0x0030, 0x003A, 0x0041, 0x005A, 0x005F, 0x005F, 0x0061, 0x007A, 0x00AA, 0x00AA,
    0x00B2, 0x00B3, 0x00B5, 0x00B5, 0x00B9, 0x00BA, 0x00BC, 0x00BE, 0x00C0, 0x00D6, 0x00D8, 0x00F6,
    0x00F8, 0x02C1, 0x02C6, 0x02D1, 0x02E0, 0x02E4, 0x02EC, 0x02EC, 0x02EE, 0x02EE, 0x0300, 0x0374,
    0x0376, 0x0377, 0x037A, 0x037D, 0x037F, 0x037F, 0x0386, 0x0386, 0x0388, 0x038A, 0x038C, 0x038C,
    0x038E, 0x03A1, 0x03A3, 0x03F5, 0x03F7, 0x0481, 0x0483, 0x052F, 0x0531, 0x0556, 0x0559, 0x0559,
    0x0560, 0x0588, 0x0591, 0x05BD, 0x05BF, 0x05BF, 0x05C1, 0x05C2, 0x05C4, 0x05C5, 0x05C7, 0x05C7,
    0x05D0, 0x05EA, 0x05EF, 0x05F2, 0x0610, 0x061A, 0x0620, 0x0669, 0x066E, 0x06D3, 0x06D5, 0x06DC,
    0x06DF, 0x06E8, 0x06EA, 0x06FC, 0x06FF, 0x06FF, 0x0710, 0x074A, 0x074D, 0x07B1, 0x07C0, 0x07F5,
    0x07FA, 0x07FA, 0x07FD, 0x07FD, 0x0800, 0x082D, 0x0840, 0x085B, 0x0860, 0x086A, 0x0870, 0x0887,
    0x0889, 0x088F, 0x0897, 0x08E1, 0x08E3, 0x0963, 0x0966, 0x096F, 0x0971, 0x0983, 0x0985, 0x098C,
    0x098F, 0x0990, 0x0993, 0x09A8, 0x09AA, 0x09B0, 0x09B2, 0x09B2, 0x09B6, 0x09B9, 0x09BC, 0x09C4,
    0x09C7, 0x09C8, 0x09CB, 0x09CE, 0x09D7, 0x09D7, 0x09DC, 0x09DD, 0x09DF, 0x09E3, 0x09E6, 0x09F1,
    0x09F4, 0x09F9, 0x09FC, 0x09FC, 0x09FE, 0x09FE, 0x0A01, 0x0A03, 0x0A05, 0x0A0A, 0x0A0F, 0x0A10,
    0x0A13, 0x0A28, 0x0A2A, 0x0A30, 0x0A32, 0x0A33, 0x0A35, 0x0A36, 0x0A38, 0x0A39, 0x0A3C, 0x0A3C,
    0x0A3E, 0x0A42, 0x0A47, 0x0A48, 0x0A4B, 0x0A4D, 0x0A51, 0x0A51, 0x0A59, 0x0A5C, 0x0A5E, 0x0A5E,
    0x0A66, 0x0A75, 0x0A81, 0x0A83, 0x0A85, 0x0A8D, 0x0A8F, 0x0A91, 0x0A93, 0x0AA8, 0x0AAA, 0x0AB0,
    0x0AB2, 0x0AB3, 0x0AB5, 0x0AB9, 0x0ABC, 0x0AC5, 0x0AC7, 0x0AC9, 0x0ACB, 0x0ACD, 0x0AD0, 0x0AD0,
    0x0AE0, 0x0AE3, 0x0AE6, 0x0AEF, 0x0AF9, 0x0AFF, 0x0B01, 0x0B03, 0x0B05, 0x0B0C, 0x0B0F, 0x0B10,
    0x0B13, 0x0B28, 0x0B2A, 0x0B30, 0x0B32, 0x0B33, 0x0B35, 0x0B39, 0x0B3C, 0x0B44, 0x0B47, 0x0B48,
    0x0B4B, 0x0B4D, 0x0B55, 0x0B57, 0x0B5C, 0x0B5D, 0x0B5F, 0x0B63, 0x0B66, 0x0B6F, 0x0B71, 0x0B77,
    0x0B82, 0x0B83, 0x0B85, 0x0B8A, 0x0B8E, 0x0B90, 0x0B92, 0x0B95, 0x0B99, 0x0B9A, 0x0B9C, 0x0B9C,
    0x0B9E, 0x0B9F, 0x0BA3, 0x0BA4, 0x0BA8, 0x0BAA, 0x0BAE, 0x0BB9, 0x0BBE, 0x0BC2, 0x0BC6, 0x0BC8,
    0x0BCA, 0x0BCD, 0x0BD0, 0x0BD0, 0x0BD7, 0x0BD7, 0x0BE6, 0x0BF2, 0x0C00, 0x0C0C, 0x0C0E, 0x0C10,
    0x0C12, 0x0C28, 0x0C2A, 0x0C39, 0x0C3C, 0x0C44, 0x0C46, 0x0C48, 0x0C4A, 0x0C4D, 0x0C55, 0x0C56,
    0x0C58, 0x0C5A, 0x0C5C, 0x0C5D, 0x0C60, 0x0C63, 0x0C66, 0x0C6F, 0x0C78, 0x0C7E, 0x0C80, 0x0C83,
    0x0C85, 0x0C8C, 0x0C8E, 0x0C90, 0x0C92, 0x0CA8, 0x0CAA, 0x0CB3, 0x0CB5, 0x0CB9, 0x0CBC, 0x0CC4,
    0x0CC6, 0x0CC8, 0x0CCA, 0x0CCD, 0x0CD5, 0x0CD6, 0x0CDC, 0x0CDE, 0x0CE0, 0x0CE3, 0x0CE6, 0x0CEF,
    0x0CF1, 0x0CF3, 0x0D00, 0x0D0C, 0x0D0E, 0x0D10, 0x0D12, 0x0D44, 0x0D46, 0x0D48, 0x0D4A, 0x0D4E,
    0x0D54, 0x0D63, 0x0D66, 0x0D78, 0x0D7A, 0x0D7F, 0x0D81, 0x0D83, 0x0D85, 0x0D96, 0x0D9A, 0x0DB1,
    0x0DB3, 0x0DBB, 0x0DBD, 0x0DBD, 0x0DC0, 0x0DC6, 0x0DCA, 0x0DCA, 0x0DCF, 0x0DD4, 0x0DD6, 0x0DD6,
    0x0DD8, 0x0DDF, 0x0DE6, 0x0DEF, 0x0DF2, 0x0DF3, 0x0E01, 0x0E3A, 0x0E40, 0x0E4E, 0x0E50, 0x0E59,
    0x0E81, 0x0E82, 0x0E84, 0x0E84, 0x0E86, 0x0E8A, 0x0E8C, 0x0EA3, 0x0EA5, 0x0EA5, 0x0EA7, 0x0EBD,
    0x0EC0, 0x0EC4, 0x0EC6, 0x0EC6, 0x0EC8, 0x0ECE, 0x0ED0, 0x0ED9, 0x0EDC, 0x0EDF, 0x0F00, 0x0F00,
    0x0F18, 0x0F19, 0x0F20, 0x0F33, 0x0F35, 0x0F35, 0x0F37, 0x0F37, 0x0F39, 0x0F39, 0x0F3E, 0x0F47,
    0x0F49, 0x0F6C, 0x0F71, 0x0F84, 0x0F86, 0x0F97, 0x0F99, 0x0FBC, 0x0FC6, 0x0FC6, 0x1000, 0x1049,
    0x1050, 0x109D, 0x10A0, 0x10C5, 0x10C7, 0x10C7, 0x10CD, 0x10CD, 0x10D0, 0x10FA, 0x10FC, 0x1248,
    0x124A, 0x124D, 0x1250, 0x1256, 0x1258, 0x1258, 0x125A, 0x125D, 0x1260, 0x1288, 0x128A, 0x128D,
    0x1290, 0x12B0, 0x12B2, 0x12B5, 0x12B8, 0x12BE, 0x12C0, 0x12C0, 0x12C2, 0x12C5, 0x12C8, 0x12D6,
    0x12D8, 0x1310, 0x1312, 0x1315, 0x1318, 0x135A, 0x135D, 0x135F, 0x1369, 0x137C, 0x1380, 0x138F,
    0x13A0, 0x13F5, 0x13F8, 0x13FD, 0x1401, 0x166C, 0x166F, 0x167F, 0x1681, 0x169A, 0x16A0, 0x16EA,
    0x16EE, 0x16F8, 0x1700, 0x1715, 0x171F, 0x1734, 0x1740, 0x1753, 0x1760, 0x176C, 0x176E, 0x1770,
    0x1772, 0x1773, 0x1780, 0x17D3, 0x17D7, 0x17D7, 0x17DC, 0x17DD, 0x17E0, 0x17E9, 0x17F0, 0x17F9,
    0x180B, 0x180D, 0x180F, 0x1819, 0x1820, 0x1878, 0x1880, 0x18AA, 0x18B0, 0x18F5, 0x1900, 0x191E,
    0x1920, 0x192B, 0x1930, 0x193B, 0x1946, 0x196D, 0x1970, 0x1974, 0x1980, 0x19AB, 0x19B0, 0x19C9,
    0x19D0, 0x19DA, 0x1A00, 0x1A1B, 0x1A20, 0x1A5E, 0x1A60, 0x1A7C, 0x1A7F, 0x1A89, 0x1A90, 0x1A99,
    0x1AA7, 0x1AA7, 0x1AB0, 0x1ADD, 0x1AE0, 0x1AEB, 0x1B00, 0x1B4C, 0x1B50, 0x1B59, 0x1B6B, 0x1B73,
    0x1B80, 0x1BF3, 0x1C00, 0x1C37, 0x1C40, 0x1C49, 0x1C4D, 0x1C7D, 0x1C80, 0x1C8A, 0x1C90, 0x1CBA,
    0x1CBD, 0x1CBF, 0x1CD0, 0x1CD2, 0x1CD4, 0x1CFA, 0x1D00, 0x1F15, 0x1F18, 0x1F1D, 0x1F20, 0x1F45,
    0x1F48, 0x1F4D, 0x1F50, 0x1F57, 0x1F59, 0x1F59, 0x1F5B, 0x1F5B, 0x1F5D, 0x1F5D, 0x1F5F, 0x1F7D,
    0x1F80, 0x1FB4, 0x1FB6, 0x1FBC, 0x1FBE, 0x1FBE, 0x1FC2, 0x1FC4, 0x1FC6, 0x1FCC, 0x1FD0, 0x1FD3,
    0x1FD6, 0x1FDB, 0x1FE0, 0x1FEC, 0x1FF2, 0x1FF4, 0x1FF6, 0x1FFC, 0x2070, 0x2071, 0x2074, 0x2079,
    0x207F, 0x2089, 0x2090, 0x209C, 0x20D0, 0x20F0, 0x2102, 0x2102, 0x2107, 0x2107, 0x210A, 0x2113,
    0x2115, 0x2115, 0x2119, 0x211D, 0x2124, 0x2124, 0x2126, 0x2126, 0x2128, 0x2128, 0x212A, 0x212D,
    0x212F, 0x2139, 0x213C, 0x213F, 0x2145, 0x2149, 0x214E, 0x214E, 0x2150, 0x2189, 0x2460, 0x249B,
    0x24EA, 0x24FF, 0x2776, 0x2793, 0x2C00, 0x2CE4, 0x2CEB, 0x2CF3, 0x2CFD, 0x2CFD, 0x2D00, 0x2D25,
    0x2D27, 0x2D27, 0x2D2D, 0x2D2D, 0x2D30, 0x2D67, 0x2D6F, 0x2D6F, 0x2D7F, 0x2D96, 0x2DA0, 0x2DA6,
    0x2DA8, 0x2DAE, 0x2DB0, 0x2DB6, 0x2DB8, 0x2DBE, 0x2DC0, 0x2DC6, 0x2DC8, 0x2DCE, 0x2DD0, 0x2DD6,
    0x2DD8, 0x2DDE, 0x2DE0, 0x2DFF, 0x2E2F, 0x2E2F, 0x3005, 0x3007, 0x3021, 0x302F, 0x3031, 0x3035,
    0x3038, 0x303C, 0x3041, 0x3096, 0x3099, 0x309A, 0x309D, 0x309F, 0x30A1, 0x30FA, 0x30FC, 0x30FF,
    0x3105, 0x312F, 0x3131, 0x318E, 0x3192, 0x3195, 0x31A0, 0x31BF, 0x31F0, 0x31FF, 0x3220, 0x3229,
    0x3248, 0x324F, 0x3251, 0x325F, 0x3280, 0x3289, 0x32B1, 0x32BF, 0x3400, 0x4DBF, 0x4E00, 0xA48C,
    0xA4D0, 0xA4FD, 0xA500, 0xA60C, 0xA610, 0xA62B, 0xA640, 0xA672, 0xA674, 0xA67D, 0xA67F, 0xA6F1,
    0xA717, 0xA71F, 0xA722, 0xA788, 0xA78B, 0xA7DC, 0xA7F1, 0xA827, 0xA82C, 0xA82C, 0xA830, 0xA835,
    0xA840, 0xA873, 0xA880, 0xA8C5, 0xA8D0, 0xA8D9, 0xA8E0, 0xA8F7, 0xA8FB, 0xA8FB, 0xA8FD, 0xA92D,
    0xA930, 0xA953, 0xA960, 0xA97C, 0xA980, 0xA9C0, 0xA9CF, 0xA9D9, 0xA9E0, 0xA9FE, 0xAA00, 0xAA36,
    0xAA40, 0xAA4D, 0xAA50, 0xAA59, 0xAA60, 0xAA76, 0xAA7A, 0xAAC2, 0xAADB, 0xAADD, 0xAAE0, 0xAAEF,
    0xAAF2, 0xAAF6, 0xAB01, 0xAB06, 0xAB09, 0xAB0E, 0xAB11, 0xAB16, 0xAB20, 0xAB26, 0xAB28, 0xAB2E,
    0xAB30, 0xAB5A, 0xAB5C, 0xAB69, 0xAB70, 0xABEA, 0xABEC, 0xABED, 0xABF0, 0xABF9, 0xAC00, 0xD7A3,
    0xD7B0, 0xD7C6, 0xD7CB, 0xD7FB, 0xF900, 0xFA6D, 0xFA70, 0xFAD9, 0xFB00, 0xFB06, 0xFB13, 0xFB17,
    0xFB1D, 0xFB28, 0xFB2A, 0xFB36, 0xFB38, 0xFB3C, 0xFB3E, 0xFB3E, 0xFB40, 0xFB41, 0xFB43, 0xFB44,
    0xFB46, 0xFBB1, 0xFBD3, 0xFD3D, 0xFD50, 0xFD8F, 0xFD92, 0xFDC7, 0xFDF0, 0xFDFB, 0xFE00, 0xFE0F,
    0xFE20, 0xFE2F, 0xFE70, 0xFE74, 0xFE76, 0xFEFC, 0xFF10, 0xFF19, 0xFF21, 0xFF3A, 0xFF41, 0xFF5A,
    0xFF66, 0xFFBE, 0xFFC2, 0xFFC7, 0xFFCA, 0xFFCF, 0xFFD2, 0xFFD7, 0xFFDA, 0xFFDC,
];

/// Whether `c` is in one of the ranges of `table`; QXmlUtils looks at UTF-16 units, so at
/// nothing past U+FFFF.
fn in_ranges(table: &[u16], c: char) -> bool {
    let Ok(c) = u16::try_from(u32::from(c)) else {
        return false;
    };
    let ranges = table.as_chunks::<2>().0;
    let found = ranges.binary_search_by(|r| match c {
        c if c < r[0] => std::cmp::Ordering::Greater,
        c if c > r[1] => std::cmp::Ordering::Less,
        _ => std::cmp::Ordering::Equal,
    });
    found.is_ok()
}

/// QXmlUtils::isNCName: a letter or `_`, then letters, digits, marks and `.`, `-`, `_`.
/// QXmlStreamReader asks it of an element's prefix and name and of the target of a
/// processing instruction with data; the name of an attribute is anything it tokenizes.
fn is_ncname(name: &str) -> bool {
    let letter = |c: char| c.is_ascii_alphabetic() || (!c.is_ascii() && in_ranges(&LETTERS, c));
    let part = |c: char| match c.is_ascii() {
        true => c.is_ascii_digit() || matches!(c, '.' | '-' | '_'),
        false => in_ranges(&NAME_MARKS, c),
    };
    name.starts_with(|c| letter(c) || c == '_') && name.chars().all(|c| letter(c) || part(c))
}

/// QXmlUtils::isEncName: what the declaration may name as the encoding.
fn is_encoding_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// A namespace declaration: its prefix and its address, which may not be one of XML's.
fn namespace_declaration(prefix: &str, address: &str) -> Option<(String, String)> {
    let legal = match prefix {
        "" => address != XML_NAMESPACE,
        "xmlns" => false,
        _ => !address.is_empty() && (prefix == "xml") == (address == XML_NAMESPACE),
    };
    (legal && address != XMLNS_NAMESPACE).then(|| (prefix.to_string(), address.to_string()))
}

/// QXmlUtils::isPublicID.
fn is_public_id(id: &str) -> bool {
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || " \r\n-'()+,./:=?;!*#@$_%".contains(c))
}

const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// An entity a document type declares.
struct Entity {
    /// `None` for an external one, whose text QXmlStreamReader does not fetch.
    value: Option<String>,
    /// The length of the value in UTF-16 units, which the expansion limit counts.
    units: i64,
    /// Declared with `NDATA`.
    unparsed: bool,
    /// One of the five XML has: its value is text whatever it holds.
    predefined: bool,
    /// It was referred to, so its value was handed on to be checked (hasBeenParsed).
    checked: bool,
    /// Its text is being read: a reference to it now is to itself.
    open: bool,
}

/// An attribute an `<!ATTLIST>` declares.
struct Declared {
    element: String,
    name: String,
    cdata: bool,
    default: Option<String>,
}

/// An open element: its node, its name and the namespaces it declares.
type Open = (usize, String, Vec<(String, String)>);

type Entities = std::collections::HashMap<String, Entity>;

/// QXmlStreamReaderPrivate for a document that is all there: its tokenizer, what the
/// document type declared and what Document::load builds of the tokens.
struct Reader {
    /// What is left to read, last character first, so that the text of an entity is put
    /// back on top of it (putStack).
    left: Vec<u32>,
    entities: Entities,
    parameters: Entities,
    /// The entities whose text is being read, a parameter entity marked (entityReferenceStack).
    referenced: Vec<(bool, String)>,
    /// How many characters the outermost of them has expanded to (entityLength).
    length: i64,
    declared: Vec<Declared>,
    standalone: bool,
    /// An external or unparsed parameter entity was referenced: later declarations do not
    /// count (referenceToUnparsedEntityDetected).
    unparsed_seen: bool,
    parameter_seen: bool,
    external_subset: bool,
    /// White space in a quoted value becomes a space: from the first start tag on
    /// (normalizeLiterals).
    in_tags: bool,
    /// This reader checks the value of an entity, which is content alone (inParseEntity).
    in_entity: bool,
    /// The entity values this reader referred to first: each has to be content by itself.
    unchecked: Vec<String>,
    /// The namespaces the XML declaration declares, for the whole document.
    namespaces: Vec<(String, String)>,
}

/// The characters one top-level entity reference may expand to (entityExpansionLimit).
const EXPANSION_LIMIT: i64 = 4096;

impl Reader {
    fn new(text: &str, in_entity: bool) -> Reader {
        let predefined = [
            ("lt", "<"),
            ("gt", ">"),
            ("amp", "&"),
            ("apos", "'"),
            ("quot", "\""),
        ];
        let entities = predefined.into_iter().map(|(name, value)| {
            let entity = Entity {
                value: Some(value.into()),
                units: 1,
                unparsed: false,
                predefined: true,
                checked: true,
                open: false,
            };
            (name.to_string(), entity)
        });
        Reader {
            left: text.chars().rev().map(u32::from).collect(),
            entities: entities.collect(),
            parameters: Entities::new(),
            referenced: vec![],
            length: 0,
            declared: vec![],
            standalone: false,
            unparsed_seen: false,
            parameter_seen: false,
            external_subset: false,
            in_tags: false,
            in_entity,
            unchecked: vec![],
            namespaces: vec![],
        }
    }

    fn get(&mut self) -> u32 {
        self.left.pop().unwrap_or(END)
    }

    fn peek(&self) -> u32 {
        self.left.last().copied().unwrap_or(END)
    }

    fn put(&mut self, c: u32) {
        if c != END {
            self.left.push(c);
        }
    }

    /// `c` as the tokenizer gives it: a carriage return, with the line feed after it if
    /// there is one, is a line feed (filterCarriageReturn).
    fn line_end(&mut self, c: char) -> char {
        if c == '\r' {
            self.is('\n');
            return '\n';
        }
        c
    }

    /// Reads `c` if it is next.
    fn is(&mut self, c: char) -> bool {
        let found = self.peek() == u32::from(c);
        if found {
            self.left.pop();
        }
        found
    }

    fn take(&mut self, c: char) -> Option<()> {
        self.is(c).then_some(())
    }

    /// Reads `word` if it is next (scanString).
    fn eat(&mut self, word: &str) -> bool {
        let next = self.left.iter().rev().copied();
        let found =
            word.len() <= self.left.len() && word.chars().map(u32::from).eq(next.take(word.len()));
        if found {
            self.left.truncate(self.left.len() - word.len());
        }
        found
    }

    /// Reads `word` and the white space after it if both are next (scanString, which wants
    /// the space for the keywords of a declaration).
    fn keyword(&mut self, word: &str) -> bool {
        let spaced =
            self.left.len() > word.len() && is_space(self.left[self.left.len() - word.len() - 1]);
        spaced && self.eat(word) && self.space()
    }

    fn space(&mut self) -> bool {
        let before = self.left.len();
        while is_space(self.peek()) {
            self.left.pop();
        }
        self.left.len() < before
    }

    /// scanUntil: what comes before `end`, which is read too. A character XML has none of
    /// is an error, and so is a marked one: a line end in the text of an entity.
    fn until(&mut self, end: &str) -> Option<String> {
        let mut out = String::new();
        loop {
            let c = self.get();
            is_token(c).then_some(())?;
            out.push(self.line_end(plain(c)?));
            if out.ends_with(end) {
                out.truncate(out.len() - end.len());
                return Some(out);
            }
        }
    }

    /// A name (fastScanName): a LETTER, then all up to white space or punctuation; a marked
    /// character and the end of an entity's text do not end it. A `prefixed` one may hold
    /// one colon, with something after it.
    fn name(&mut self, prefixed: bool) -> Option<String> {
        let first = self.get();
        is_letter(first).then_some(())?;
        let mut name = String::from(letter_of(first));
        let mut colon = false;
        for _ in 0..4096 {
            let c = self.get();
            match plain(c) {
                _ if c == END => return None,
                Some(
                    '\n' | ' ' | '\t' | '\r' | '&' | '#' | '\'' | '"' | '<' | '>' | '[' | ']' | '='
                    | '%' | '/' | ';' | '?' | '!' | '^' | '|' | ',' | '(' | ')' | '+' | '*',
                ) => {
                    self.put(c);
                    if prefixed && name.ends_with(':') {
                        name.pop();
                        self.put(u32::from(':'));
                    }
                    return Some(name);
                }
                Some(':') if !prefixed || colon => {
                    self.put(c);
                    return Some(name);
                }
                Some(':') => {
                    colon = true;
                    name.push(':');
                }
                _ => name.push(letter_of(c)),
            }
        }
        None
    }

    /// entitiesMustBeDeclared: a reference to an entity nobody declared is an error, unless
    /// declarations may have been left unread: after a reference to a parameter entity or
    /// with an external subset, in a document that is not standalone.
    fn strict(&self) -> bool {
        !self.in_entity
            && (self.standalone
                || !(self.unparsed_seen || self.parameter_seen || self.external_subset))
    }

    /// referenceEntity: starts reading the text of an entity, which may not be open and may
    /// not take the outermost reference past the limit. Its end is marked in the input.
    fn refer(&mut self, parameter: bool, name: &str) -> Option<()> {
        let table = if parameter {
            &mut self.parameters
        } else {
            &mut self.entities
        };
        let entity = table.get_mut(name)?;
        (!entity.open).then_some(())?;
        let reference = match self.referenced.is_empty() {
            true => 0,
            false => name.encode_utf16().count() as i64 + 2,
        };
        self.length += entity.units - reference;
        (self.length <= EXPANSION_LIMIT).then_some(())?;
        entity.open = true;
        self.referenced.push((parameter, name.to_string()));
        self.left.push(DONE);
        Some(())
    }

    /// entity_done: the text of the innermost open entity was read.
    fn done(&mut self) -> Option<()> {
        let (parameter, name) = self.referenced.pop()?;
        let table = if parameter {
            &mut self.parameters
        } else {
            &mut self.entities
        };
        table.get_mut(&name)?.open = false;
        if self.referenced.is_empty() {
            self.length = 0;
        }
        Some(())
    }

    /// A character reference after `&#`: its character is put back as text. `x` starts a
    /// hexadecimal one, which QString::toUInt also takes with `0x` before it.
    fn character(&mut self) -> Option<()> {
        let mut digits = String::new();
        loop {
            let c = self.get();
            if c == u32::from(';') {
                break;
            }
            (is_letter(c) || plain(c).is_some_and(|c| c.is_ascii_digit())).then_some(())?;
            digits.push(letter_of(c));
        }
        let all_digits = |s: &str, radix| !s.is_empty() && s.chars().all(|c| c.is_digit(radix));
        let code = match digits.strip_prefix('x') {
            Some(hex) => {
                let hex = hex
                    .strip_prefix("0x")
                    .or(hex.strip_prefix("0X"))
                    .unwrap_or(hex);
                all_digits(hex, 16).then_some(())?;
                u32::from_str_radix(hex, 16).ok()?
            }
            None => {
                all_digits(&digits, 10).then_some(())?;
                digits.parse().ok()?
            }
        };
        let valid =
            matches!(code, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF);
        valid.then_some(())?;
        self.left.push(TEXT | code);
        Some(())
    }

    /// A reference to the entity `name`, declared: its text is put back to be read next.
    /// In content it is read as markup again, only its line ends are text (putReplacement);
    /// in an attribute value all of it is text but its references, and a line end is a
    /// space (putReplacementInAttributeValue). The first reference has the value checked.
    fn expand(&mut self, name: &str, in_value: bool) -> Option<()> {
        let entity = &self.entities[name];
        (!entity.unparsed && !(in_value && entity.value.is_none())).then_some(())?;
        if entity.predefined {
            let value = entity.value.as_deref().unwrap_or_default();
            self.left
                .extend(value.chars().rev().map(|c| TEXT | u32::from(c)));
            return Some(());
        }
        if !entity.checked {
            // parseEntity reads the value with a reader of its own here; an error there is
            // an error of the document whenever it is found, so `parse` reads it afterwards.
            self.unchecked
                .extend(entity.value.clone().filter(|v| !v.is_empty()));
            self.entities.get_mut(name)?.checked = true;
        }
        self.refer(false, name)?;
        let value = self.entities[name].value.as_deref().unwrap_or_default();
        self.left.extend(value.chars().rev().map(|c| match c {
            '\n' | '\r' if in_value => u32::from(' '),
            '&' | ';' if in_value => u32::from(c),
            '\n' | '\r' => TEXT | u32::from(c),
            _ if in_value => TEXT | u32::from(c),
            _ => u32::from(c),
        }));
        Some(())
    }

    /// The rest of a text after its first character (fastScanContentCharList): up to markup,
    /// a reference or a character XML has none of. `]]>` is an error.
    fn text(&mut self, text: &mut String) -> Option<()> {
        loop {
            let c = self.get();
            let marked = c & TEXT != 0;
            match letter_of(c) {
                ']' => {
                    text.push(']');
                    while self.is(']') {
                        text.push(']');
                    }
                    (!(self.peek() == u32::from('>') && text.ends_with("]]"))).then_some(())?;
                }
                // A line end, which a carriage return from a reference is here.
                '\r' => {
                    self.is('\n');
                    text.push('\n');
                }
                x @ ('\n' | ' ' | '\t') => text.push(x),
                '\0' | '\u{fffe}' | '\u{ffff}' => break self.put(c),
                x if !marked && (x == '&' || x == '<' || x < ' ') => break self.put(c),
                x => text.push(x),
            }
        }
        Some(())
    }

    /// A quoted value (attribute_value, or with no `references` a literal): `None` inside
    /// for `""`, which as a default declares none. White space is as written before the
    /// first start tag and a space after it, except right after a reference
    /// (fastScanLiteralContent normalizes, the tokens before it do not).
    fn value(&mut self, references: bool) -> Option<Option<String>> {
        let quote = self.get();
        (quote == u32::from('"') || quote == u32::from('\'')).then_some(())?;
        if self.peek() == quote {
            self.left.pop();
            return Some(None);
        }
        let mut out = String::new();
        loop {
            let c = self.get();
            match plain(c) {
                _ if c == quote => return Some(Some(out)),
                _ if c == DONE => {
                    references.then_some(())?;
                    self.done()?;
                    continue;
                }
                Some('&') => {
                    references.then_some(())?;
                    if self.is('#') {
                        self.character()?;
                        continue;
                    }
                    let name = self.name(false)?;
                    self.take(';')?;
                    if self.entities.contains_key(&name) {
                        self.expand(&name, true)?;
                    } else {
                        // Left as written where it need not be declared.
                        (!self.strict()).then_some(())?;
                        out.extend(['&'].into_iter().chain(name.chars()).chain([';']));
                    }
                    continue;
                }
                Some('<') => return None,
                Some(x @ (' ' | '\t' | '\n' | '\r')) => {
                    let x = self.line_end(x);
                    out.push(if self.in_tags { ' ' } else { x });
                }
                _ => {
                    is_token(c).then_some(())?;
                    out.push(letter_of(c));
                }
            }
            loop {
                let c = self.get();
                let marked = c & TEXT != 0;
                match letter_of(c) {
                    x @ ('\n' | ' ' | '\t' | '\r') => {
                        if x == '\r' {
                            self.is('\n');
                        }
                        out.push(if self.in_tags { ' ' } else { x });
                    }
                    '\0' | '\u{fffe}' | '\u{ffff}' => break self.put(c),
                    x if !marked && (matches!(x, '&' | '<' | '"' | '\'') || x < ' ') => {
                        break self.put(c);
                    }
                    x => out.push(x),
                }
            }
        }
    }

    /// The XML declaration, if the document starts with one (xml_decl and startDocument):
    /// `version="1.0"`, then attributes as a tag has them. `encoding` is the name of an
    /// encoding QStringDecoder has and comes before `standalone`, which is `yes` or `no`;
    /// of each the last one counts. `xmlns`
    /// and `xmlns:prefix` declare a namespace for the whole document, as the tokenizer
    /// takes them for what they are in a tag; any other attribute is an error.
    fn declaration(&mut self, doc: &mut Doc) -> Option<()> {
        if !self.keyword("<?xml") {
            return Some(());
        }
        self.eat("version").then_some(())?;
        self.space();
        self.take('=')?;
        self.space();
        doc.version = self.value(false)?.unwrap_or_default();
        (doc.version == "1.0").then_some(())?;
        let mut standalone = None;
        let mut spaced = self.space();
        while !self.is('?') {
            spaced.then_some(())?;
            let name = self.name(true)?;
            self.space();
            self.take('=')?;
            self.space();
            let value = self.value(true)?.unwrap_or_default();
            match name.as_str() {
                "xmlns" => self.namespaces.push(namespace_declaration("", &value)?),
                "encoding" => {
                    (standalone.is_none() && is_encoding_name(&value)).then_some(())?;
                    crate::xhr_text::charset(value.as_bytes())?;
                    doc.encoding = value;
                }
                "standalone" => {
                    standalone = Some(match value.as_str() {
                        "yes" => true,
                        "no" => false,
                        _ => return None,
                    })
                }
                _ => {
                    let prefix = name.strip_prefix("xmlns:")?;
                    self.namespaces.push(namespace_declaration(prefix, &value)?);
                }
            }
            spaced = self.space();
        }
        self.take('>')?;
        self.standalone = standalone == Some(true);
        doc.standalone = self.standalone;
        Some(())
    }

    /// A processing instruction after `<?`: its target is no `xml`, and a name without a
    /// colon when data follows it.
    fn instruction(&mut self) -> Option<()> {
        let target = self.name(false)?;
        let xml = target.eq_ignore_ascii_case("xml");
        if self.space() {
            self.until("?>")?;
            (!xml && is_ncname(&target)).then_some(())
        } else {
            self.take('?')?;
            self.take('>')?;
            (!xml).then_some(())
        }
    }

    /// A start tag after `<`: its element with its attributes (resolveTag). The document
    /// type gives an attribute it declares as no CDATA single spaces, and after the written
    /// ones the defaults that were not written; a default `xmlns` declares a namespace even
    /// where the tag writes one.
    fn tag(&mut self, doc: &mut Doc, open: &mut Vec<Open>) -> Option<()> {
        self.in_tags = true;
        let name = self.name(true)?;
        let split = |name: &str| match name.split_once(':') {
            Some((prefix, local)) => (Some(prefix.to_string()), local.to_string()),
            None => (None, name.to_string()),
        };
        let (prefix, local) = split(&name);
        (prefix.as_deref().is_none_or(is_ncname) && is_ncname(&local)).then_some(())?;
        let mut pairs: Vec<(String, String)> = vec![];
        let mut namespaces: Vec<(String, String)> = vec![];
        let mut spaced = self.space();
        let empty = loop {
            if self.is('/') {
                self.take('>')?;
                break true;
            }
            if self.is('>') {
                break false;
            }
            spaced.then_some(())?;
            let k = self.name(true)?;
            self.space();
            self.take('=')?;
            self.space();
            let mut v = self.value(true)?.unwrap_or_default();
            if k == "xmlns" {
                namespaces.push(namespace_declaration("", &v)?);
            } else {
                let tokens = |d: &Declared| !d.cdata && d.element == name && d.name == k;
                if self.declared.iter().any(tokens) {
                    v = v
                        .split(' ')
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ");
                }
                match k.strip_prefix("xmlns:") {
                    Some(prefix) => namespaces.push(namespace_declaration(prefix, &v)?),
                    None => pairs.push((k, v)),
                }
            }
            spaced = self.space();
        };
        let defaults = |d: &&Declared| d.element == name && d.default.is_some();
        for d in self.declared.iter().filter(defaults) {
            let address = d.default.as_deref().unwrap_or_default();
            match d.name.strip_prefix("xmlns") {
                Some("") => namespaces.push(namespace_declaration("", address)?),
                Some(prefix) if prefix.starts_with(':') => {
                    namespaces.push(namespace_declaration(&prefix[1..], address)?)
                }
                _ if pairs.iter().all(|p| p.0 != d.name) => {
                    pairs.push((d.name.clone(), address.to_string()))
                }
                _ => {}
            }
        }
        let id = doc.nodes.len();
        let parent = open.last().map(|o| o.0);
        open.push((id, name, namespaces));
        // The namespace of `prefix`, which an attribute without one does not have.
        let namespace = |prefix: Option<&str>, attribute: bool| match prefix {
            None if attribute => Some(String::new()),
            _ => open
                .iter()
                .rev()
                .flat_map(|o| o.2.iter().rev())
                .chain(self.namespaces.iter().rev())
                .find(|d| d.0 == prefix.unwrap_or(""))
                .map(|d| d.1.clone())
                .or_else(|| (prefix == Some("xml")).then(|| XML_NAMESPACE.to_string()))
                .or_else(|| prefix.is_none().then(String::new)),
        };
        let mut element = Node {
            kind: Kind::Element,
            name: local,
            namespace: namespace(prefix.as_deref(), false)?,
            data: String::new(),
            parent,
            children: vec![],
            attributes: vec![],
        };
        let mut attributes = vec![];
        let mut seen = std::collections::HashSet::new();
        for (i, (k, v)) in pairs.into_iter().enumerate() {
            let (prefix, local) = split(&k);
            let namespace = namespace(prefix.as_deref(), true)?;
            // Twice the same name in the same namespace, whatever the prefixes.
            seen.insert((namespace.clone(), local.clone()))
                .then_some(())?;
            element.attributes.push(id + 1 + i);
            attributes.push(Node {
                kind: Kind::Attr,
                name: local,
                namespace,
                data: v,
                parent: Some(id),
                children: vec![],
                attributes: vec![],
            });
        }
        doc.nodes.push(element);
        doc.nodes.extend(attributes);
        match parent {
            Some(parent) => doc.nodes[parent].children.push(id),
            None => doc.root = id,
        }
        if empty {
            open.pop();
        }
        Some(())
    }

    /// The document after its declaration: the tokens QXmlStreamReader reads and the nodes
    /// Document::load makes of them. A reader of an entity's value reads content alone.
    fn body(&mut self, doc: &mut Doc) -> Option<()> {
        let mut open: Vec<Open> = vec![];
        let (mut rooted, mut typed) = (false, false);
        let mut text = String::new();
        // A text ends: at markup, and at a reference QXmlStreamReader passes on unread.
        let flush = |doc: &mut Doc, open: &[Open], text: &mut String| {
            if let Some((parent, ..)) = open.last()
                && !text.is_empty()
            {
                push(doc, Kind::Text, *parent, std::mem::take(text));
            }
            text.clear();
        };
        loop {
            let c = self.get();
            // Around the root there is only white space and markup.
            let prolog = !self.in_entity && open.is_empty();
            if c == u32::from('<') {
                flush(doc, &open, &mut text);
                if self.is('!') {
                    if self.eat("[CDATA[") {
                        (!prolog).then_some(())?;
                        let data = self.until("]]>")?;
                        if let Some((parent, ..)) = open.last() {
                            push(doc, Kind::Cdata, *parent, data);
                        }
                    } else if self.eat("--") {
                        self.until("--")?;
                        self.take('>')?;
                    } else if self.keyword("DOCTYPE") {
                        // One, before the root; the reader of an entity's value has no say.
                        (self.in_entity || (prolog && !rooted && !typed)).then_some(())?;
                        typed = true;
                        self.doctype()?;
                    } else {
                        return None;
                    }
                } else if self.is('?') {
                    self.instruction()?;
                } else if self.is('/') {
                    let (_, name, _) = open.pop()?;
                    (self.name(true)? == name).then_some(())?;
                    self.space();
                    self.take('>')?;
                } else {
                    (!(prolog && rooted)).then_some(())?;
                    self.tag(doc, &mut open)?;
                    rooted = true;
                }
            } else if c == END {
                flush(doc, &open, &mut text);
                return (open.is_empty() && (rooted || self.in_entity)).then_some(());
            } else if prolog {
                is_space(c).then_some(())?;
            } else if c == DONE {
                self.done()?;
            } else if c == u32::from('&') {
                if self.is('#') {
                    self.character()?;
                    continue;
                }
                let name = self.name(false)?;
                self.take(';')?;
                if self.entities.contains_key(&name) {
                    self.expand(&name, false)?;
                } else {
                    (!self.strict()).then_some(())?;
                    flush(doc, &open, &mut text);
                }
            } else {
                is_token(c).then_some(())?;
                text.push(match plain(c) {
                    Some(c) => self.line_end(c),
                    None => letter_of(c),
                });
                self.text(&mut text)?;
            }
        }
    }

    /// `<!DOCTYPE name` and what follows it: an external identifier, the internal subset.
    fn doctype(&mut self) -> Option<()> {
        self.name(true)?;
        if self.is('>') {
            return Some(());
        }
        if self.peek() != u32::from('[') {
            self.space().then_some(())?;
            self.external()
                .map(|external| self.external_subset = external)?;
            self.space();
        }
        if self.is('[') {
            self.subset()?;
            self.space();
        }
        self.take('>')
    }

    /// `SYSTEM "address"` or `PUBLIC "name" "address"` if one is next: whether it was.
    fn external(&mut self) -> Option<bool> {
        if self.keyword("SYSTEM") {
            self.value(false)?;
        } else if self.keyword("PUBLIC") {
            is_public_id(&self.value(false)?.unwrap_or_default()).then_some(())?;
            self.space().then_some(())?;
            self.value(false)?;
        } else {
            return Some(false);
        }
        Some(true)
    }

    /// The internal subset after `[`: declarations, comments, instructions, white space and
    /// references to parameter entities, whose text is read as more of it.
    fn subset(&mut self) -> Option<()> {
        loop {
            let c = self.get();
            match plain(c) {
                Some(']') => return Some(()),
                Some(' ' | '\t' | '\n' | '\r') => {}
                _ if c == DONE => self.done()?,
                Some('%') => {
                    let name = self.name(false)?;
                    self.take(';')?;
                    match self.parameters.get(&name) {
                        Some(entity) => {
                            self.parameter_seen = true;
                            if entity.value.is_none() {
                                self.unparsed_seen = true;
                            } else {
                                self.refer(true, &name)?;
                                let value =
                                    self.parameters[&name].value.as_deref().unwrap_or_default();
                                self.left.extend(value.chars().rev().map(u32::from));
                            }
                        }
                        None => (!self.strict()).then_some(())?,
                    }
                }
                Some('<') if self.is('?') => self.instruction()?,
                Some('<') => {
                    self.take('!')?;
                    if self.eat("--") {
                        self.until("--")?;
                        self.take('>')?;
                    } else if self.keyword("ELEMENT") {
                        self.element()?;
                    } else if self.keyword("ENTITY") {
                        self.entity()?;
                    } else if self.keyword("ATTLIST") {
                        self.attlist()?;
                    } else if self.keyword("NOTATION") {
                        self.notation()?;
                    } else {
                        return None;
                    }
                }
                _ => return None,
            }
        }
    }

    /// `<!ELEMENT name` and its content model, which only has to be one.
    fn element(&mut self) -> Option<()> {
        self.name(true)?;
        self.space().then_some(())?;
        let quantified = |r: &mut Reader| r.is('?') || r.is('*') || r.is('+');
        if !(self.eat("EMPTY") || self.eat("ANY")) {
            self.take('(')?;
            self.space();
            if self.is('#') {
                self.eat("PCDATA").then_some(())?;
                self.space();
                let mut names = false;
                while self.is('|') {
                    self.space();
                    self.name(true)?;
                    self.space();
                    names = true;
                }
                self.take(')')?;
                (self.is('*') || !names).then_some(())?;
            } else {
                // What separates the parts of each open group, once it is known.
                let mut groups = vec![END];
                while !groups.is_empty() {
                    self.space();
                    if self.is('(') {
                        groups.push(END);
                        continue;
                    }
                    self.name(true)?;
                    quantified(self);
                    // After a part: the group goes on or ends, and is a part itself then.
                    while let Some(separator) = groups.last_mut() {
                        self.space();
                        let c = self.get();
                        if c == u32::from(')') {
                            groups.pop();
                            quantified(self);
                        } else if (c == u32::from('|') || c == u32::from(','))
                            && (*separator == END || *separator == c)
                        {
                            *separator = c;
                            break;
                        } else {
                            return None;
                        }
                    }
                }
            }
        }
        self.space();
        self.take('>')
    }

    /// `<!ENTITY name "value">`, `<!ENTITY % name "value">` or one with an external
    /// identifier: the first declaration of a name counts, and none after a reference to an
    /// external parameter entity unless the document is standalone.
    fn entity(&mut self) -> Option<()> {
        let parameter = self.is('%');
        if parameter {
            self.space().then_some(())?;
        }
        let name = self.name(false)?;
        self.space().then_some(())?;
        let mut unparsed = false;
        let value = if self.external()? {
            if self.space() && self.keyword("NDATA") {
                self.name(false)?;
                (!parameter).then_some(())?;
                unparsed = true;
            }
            None
        } else {
            Some(self.entity_value()?)
        };
        self.space();
        self.take('>')?;
        if self.unparsed_seen && !self.standalone {
            return Some(());
        }
        let table = if parameter {
            &mut self.parameters
        } else {
            &mut self.entities
        };
        table.entry(name).or_insert(Entity {
            units: value
                .as_deref()
                .map_or(0, |v| v.encode_utf16().count() as i64),
            value,
            unparsed,
            predefined: false,
            checked: false,
            open: false,
        });
        Some(())
    }

    /// The quoted value of an entity: its character references are replaced, its entity
    /// references stay; a `%` is an error.
    fn entity_value(&mut self) -> Option<String> {
        let quote = self.get();
        (quote == u32::from('"') || quote == u32::from('\'')).then_some(())?;
        let mut out = String::new();
        loop {
            let c = self.get();
            match plain(c) {
                _ if c == quote => return Some(out),
                _ if c == DONE => self.done()?,
                Some('%') => return None,
                Some('&') if self.is('#') => self.character()?,
                Some('&') => {
                    let name = self.name(false)?;
                    self.take(';')?;
                    out.extend(['&'].into_iter().chain(name.chars()).chain([';']));
                }
                Some('\r') => out.push(self.line_end('\r')),
                _ => {
                    is_token(c).then_some(())?;
                    out.push(letter_of(c));
                }
            }
        }
    }

    /// `<!ATTLIST element` and the attributes it declares: name, type, default. The first
    /// declaration of an attribute counts; a default that is no CDATA is simplified.
    fn attlist(&mut self) -> Option<()> {
        let element = self.name(true)?;
        let mut found: Vec<Declared> = vec![];
        loop {
            let spaced = self.space();
            if self.is('>') {
                break;
            }
            spaced.then_some(())?;
            let name = self.name(true)?;
            self.space().then_some(())?;
            let cdata = self.keyword("CDATA");
            let tokens = [
                "ID", "IDREF", "IDREFS", "ENTITY", "ENTITIES", "NMTOKEN", "NMTOKENS",
            ];
            if !cdata && !tokens.iter().any(|word| self.keyword(word)) {
                // An enumeration, of notations too.
                self.keyword("NOTATION");
                self.take('(')?;
                loop {
                    self.space();
                    let first = self.get();
                    // A name token: a LETTER, a digit or one of `.`, `-`, `:`, then its parts.
                    // Of a character past U+FFFF the second UTF-16 unit would have to be one.
                    let start = |c: char| c.is_ascii_digit() || matches!(c, '.' | '-' | ':');
                    (is_letter(first) || plain(first).is_some_and(start)).then_some(())?;
                    (letter_of(first) <= '\u{ffff}').then_some(())?;
                    while self.peek() & (DONE | END) == 0
                        && in_ranges(&TOKEN_PARTS, letter_of(self.peek()))
                    {
                        self.left.pop();
                    }
                    self.space();
                    if !self.is('|') {
                        break;
                    }
                }
                self.take(')')?;
                self.space().then_some(())?;
            }
            let default = if self.is('#') {
                if self.eat("REQUIRED") || self.eat("IMPLIED") {
                    None
                } else {
                    self.eat("FIXED").then_some(())?;
                    self.space().then_some(())?;
                    self.value(true)?
                }
            } else {
                self.value(true)?
            };
            let default = match cdata {
                true => default,
                false => default.map(|v| v.split_whitespace().collect::<Vec<_>>().join(" ")),
            };
            found.push(Declared {
                element: element.clone(),
                name,
                cdata,
                default,
            });
        }
        if self.unparsed_seen && !self.standalone {
            return Some(());
        }
        for d in found {
            let same = |have: &Declared| have.element == d.element && have.name == d.name;
            if !self.declared.iter().any(same) {
                self.declared.push(d);
            }
        }
        Some(())
    }

    /// `<!NOTATION name` with a system or a public identifier.
    fn notation(&mut self) -> Option<()> {
        self.name(false)?;
        self.space().then_some(())?;
        if self.keyword("SYSTEM") {
            self.value(false)?;
        } else if self.keyword("PUBLIC") {
            is_public_id(&self.value(false)?.unwrap_or_default()).then_some(())?;
            if self.space() && self.peek() != u32::from('>') {
                self.value(false)?;
            }
        } else {
            return None;
        }
        self.space();
        self.take('>')
    }
}

/// Document::load: the tree of `data`, or `None` where QXmlStreamReader reports an error.
/// Comments, processing instructions and the document type leave no node.
pub(crate) fn parse(data: &[u8]) -> Option<Doc> {
    let text = text(data)?;
    let mut doc = Doc::empty();
    let mut reader = Reader::new(&text, false);
    reader.declaration(&mut doc)?;
    reader.body(&mut doc)?;
    // parseEntity: each value has a reader of its own, which may refer to entities of a
    // document type inside the value. One after the other, however deep they go.
    let mut unchecked = reader.unchecked;
    while let Some(value) = unchecked.pop() {
        let mut inner = Reader::new(&value, true);
        inner.body(&mut Doc::empty())?;
        unchecked.append(&mut inner.unchecked);
    }
    Some(doc)
}

fn push(doc: &mut Doc, kind: Kind, parent: usize, data: String) {
    let id = doc.nodes.len();
    doc.nodes.push(Node {
        kind,
        name: String::new(),
        namespace: String::new(),
        data,
        parent: Some(parent),
        children: vec![],
        attributes: vec![],
    });
    doc.nodes[parent].children.push(id);
}

/// The prototypes of the DOM objects of one engine, kept by its `XMLHttpRequest`.
pub(crate) struct Protos {
    element: Obj,
    attr: Obj,
    text: Obj,
    cdata: Obj,
    document: Obj,
}

/// What a DOM object stands for: a node of `doc`, or with `None` the document itself.
struct Ref {
    doc: Rc<Doc>,
    id: Option<usize>,
}

impl Ref {
    fn node(&self) -> Option<&Node> {
        self.id.map(|id| &self.doc.nodes[id])
    }

    fn kind(&self) -> Kind {
        self.node().map_or(Kind::Document, |n| n.kind)
    }

    fn data(&self) -> &str {
        self.node().map_or("", |n| &n.data)
    }

    /// The nodes beside this one, and where it is among them.
    fn siblings(&self) -> Option<(&[usize], usize)> {
        let among = &self.doc.nodes[self.node()?.parent?].children;
        Some((among, among.iter().position(|c| Some(*c) == self.id)?))
    }
}

fn node_of(this: &Value) -> Option<Rc<Ref>> {
    match this {
        Value::Obj(o) => o.borrow().native.clone()?.downcast::<Ref>().ok(),
        _ => None,
    }
}

fn protos(it: &Interp) -> Option<Rc<Protos>> {
    match it.global.borrow().vars.get("XMLHttpRequest") {
        Some(Value::Obj(c)) => c.borrow().native.clone()?.downcast::<Protos>().ok(),
        _ => None,
    }
}

/// Node::create: the object for node `id` of `doc`, a new one each time.
fn wrap(it: &Interp, doc: &Rc<Doc>, id: Option<usize>) -> Value {
    let Some(protos) = protos(it) else {
        return Value::Undefined;
    };
    let r = Ref {
        doc: doc.clone(),
        id,
    };
    let proto = match r.kind() {
        Kind::Element => &protos.element,
        Kind::Attr => &protos.attr,
        Kind::Text => &protos.text,
        Kind::Cdata => &protos.cdata,
        Kind::Document => &protos.document,
    };
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        b.proto = Some(proto.clone());
        b.native = Some(Rc::new(r));
    }
    Value::Obj(o)
}

/// The document object of `doc`.
pub(crate) fn document(it: &Interp, doc: Doc) -> Value {
    wrap(it, &Rc::new(doc), None)
}

fn wrap_or_null(it: &Interp, r: &Ref, id: Option<usize>) -> Value {
    id.map_or(Value::Null, |id| wrap(it, &r.doc, Some(id)))
}

/// A NodeList or a NamedNodeMap of `ids`: the nodes by index, `length`, and with `named`
/// each by its name. None of them is listed among the object's keys.
fn list(it: &Interp, r: &Ref, ids: &[usize], named: bool) -> Value {
    let o = new_obj();
    for (i, id) in ids.iter().enumerate() {
        let node = wrap(it, &r.doc, Some(*id));
        let name = &r.doc.nodes[*id].name;
        if named && !crate::eval::has_own(&o, name) {
            set_hidden(&o, name, node.clone());
        }
        set_hidden(&o, &i.to_string(), node);
    }
    set_hidden(&o, "length", Value::Num(ids.len() as f64));
    Value::Obj(o)
}

/// A getter of the Node prototype: any other `this` is a TypeError.
fn of_node(it: &Interp, this: &Value, f: impl FnOnce(&Ref) -> Value) -> R<Value> {
    match node_of(this) {
        Some(r) => Ok(f(&r)),
        None => it.throw("TypeError", "Type error"),
    }
}

/// A getter of the other prototypes: any other `this` reads `undefined`.
fn of_other(this: &Value, f: impl FnOnce(&Ref) -> Value) -> R<Value> {
    Ok(node_of(this).map_or(Value::Undefined, |r| f(&r)))
}

fn node_name(it: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    of_node(it, this, |r| {
        Value::str(match r.kind() {
            Kind::Document => "#document",
            Kind::Cdata => "#cdata-section",
            Kind::Text => "#text",
            _ => r.node().map_or("", |n| &n.name),
        })
    })
}

fn node_value(it: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    of_node(it, this, |r| match r.kind() {
        Kind::Document | Kind::Element => Value::Null,
        _ => Value::str(r.data()),
    })
}

fn proto_with(parent: Option<&Obj>, getters: &[(&'static str, NativeFn)]) -> Obj {
    let o = new_obj();
    o.borrow_mut().proto = parent.cloned();
    for (k, f) in getters {
        define_accessor(&o, k, true, fn_value(Callable::Native(k, *f)), false);
    }
    o
}

/// The prototypes: Node under Element, Attr, Document and CharacterData, that one under
/// Text, and Text under CDATA.
pub(crate) fn prototypes() -> Protos {
    let node = proto_with(
        None,
        &[
            ("nodeName", node_name),
            ("nodeValue", node_value),
            ("nodeType", |it, this, _| {
                of_node(it, this, |r| Value::Num(f64::from(r.kind() as u8)))
            }),
            ("namespaceUri", |it, this, _| {
                of_node(it, this, |r| {
                    Value::str(r.node().map_or("", |n| &n.namespace))
                })
            }),
            ("parentNode", |it, this, _| {
                of_node(it, this, |r| {
                    wrap_or_null(it, r, r.node().and_then(|n| n.parent))
                })
            }),
            ("childNodes", |it, this, _| {
                of_node(it, this, |r| {
                    list(it, r, r.node().map_or(&[], |n| &n.children), false)
                })
            }),
            ("firstChild", |it, this, _| {
                of_node(it, this, |r| {
                    wrap_or_null(it, r, r.node().and_then(|n| n.children.first().copied()))
                })
            }),
            ("lastChild", |it, this, _| {
                of_node(it, this, |r| {
                    wrap_or_null(it, r, r.node().and_then(|n| n.children.last().copied()))
                })
            }),
            ("previousSibling", |it, this, _| {
                of_node(it, this, |r| {
                    let id = r
                        .siblings()
                        .and_then(|(among, at)| among.get(at.checked_sub(1)?).copied());
                    wrap_or_null(it, r, id)
                })
            }),
            ("nextSibling", |it, this, _| {
                of_node(it, this, |r| {
                    let id = r
                        .siblings()
                        .and_then(|(among, at)| among.get(at + 1).copied());
                    wrap_or_null(it, r, id)
                })
            }),
            ("attributes", |it, this, _| {
                of_node(it, this, |r| match r.node() {
                    Some(n) if n.kind == Kind::Element => list(it, r, &n.attributes, true),
                    _ => Value::Null,
                })
            }),
        ],
    );
    let character_data = proto_with(
        Some(&node),
        &[
            ("data", node_value),
            ("length", |_, this, _| {
                of_other(this, |r| Value::Num(r.data().encode_utf16().count() as f64))
            }),
        ],
    );
    let text = proto_with(
        Some(&character_data),
        &[
            ("isElementContentWhitespace", |_, this, _| {
                of_other(this, |r| Value::Bool(r.data().trim().is_empty()))
            }),
            ("wholeText", |_, this, _| {
                of_other(this, |r| Value::str(r.data()))
            }),
        ],
    );
    Protos {
        element: proto_with(Some(&node), &[("tagName", node_name)]),
        attr: proto_with(
            Some(&node),
            &[
                ("name", |_, this, _| {
                    of_other(this, |r| Value::str(r.node().map_or("", |n| &n.name)))
                }),
                ("value", |_, this, _| {
                    of_other(this, |r| Value::str(r.data()))
                }),
                ("ownerElement", |it, this, _| {
                    of_other(this, |r| {
                        wrap_or_null(it, r, r.node().and_then(|n| n.parent))
                    })
                }),
            ],
        ),
        cdata: proto_with(Some(&text), &[]),
        text,
        document: proto_with(
            Some(&node),
            &[
                ("xmlVersion", |_, this, _| {
                    of_document(this, |d| Value::str(&d.version))
                }),
                ("xmlEncoding", |_, this, _| {
                    of_document(this, |d| Value::str(&d.encoding))
                }),
                ("xmlStandalone", |_, this, _| {
                    of_document(this, |d| Value::Bool(d.standalone))
                }),
                ("documentElement", |it, this, _| {
                    Ok(match node_of(this) {
                        Some(r) if r.id.is_none() => wrap(it, &r.doc, Some(r.doc.root)),
                        _ => Value::Undefined,
                    })
                }),
            ],
        ),
    }
}

/// A getter of the Document prototype: `undefined` on anything but a document.
fn of_document(this: &Value, f: impl FnOnce(&Doc) -> Value) -> R<Value> {
    Ok(match node_of(this) {
        Some(r) if r.id.is_none() => f(&r.doc),
        _ => Value::Undefined,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A converter of ICU that is not written here (docs/qt-differences.md): the document
    /// is not read again, it stays in the encoding it was read in up to its declaration.
    /// A name nobody knows ends the document, as in Qt.
    #[test]
    fn a_declaration_that_names_a_charset_not_written_here_leaves_the_text_as_it_was_read() {
        let named = |name: &str| format!("<?xml version=\"1.0\" encoding=\"{name}\"?><a>é</a>");
        assert_eq!(text(named("IBM437").as_bytes()), Some(named("IBM437")));
        let wide: Vec<u8> = named("IBM437")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(
            text(&[&[0xFF, 0xFE], &wide[..]].concat()),
            Some(named("IBM437"))
        );
        assert!(parse(named("IBM437").as_bytes()).is_some());
        assert!(parse(named("x-nobody-knows").as_bytes()).is_none());
        assert_eq!(
            text(b"<?xml version=\"1.0\" encoding=\"koi8-r\"?><a>\xf0</a>"),
            Some(named("koi8-r").replace('é', "П"))
        );
    }

    /// The tree of `xml` as nested lists: an element's name, attributes and children, a
    /// text in quotes, a CDATA section in brackets.
    fn tree(xml: &str) -> Option<String> {
        fn show(doc: &Doc, id: usize) -> String {
            let n = &doc.nodes[id];
            match n.kind {
                Kind::Text => format!("{:?}", n.data),
                Kind::Cdata => format!("[{}]", n.data),
                _ => {
                    let attributes: String = n
                        .attributes
                        .iter()
                        .map(|a| format!(" {}={:?}", doc.nodes[*a].name, doc.nodes[*a].data))
                        .collect();
                    let children: String = n
                        .children
                        .iter()
                        .map(|c| format!(" {}", show(doc, *c)))
                        .collect();
                    format!("({}{attributes}{children})", n.name)
                }
            }
        }
        let doc = parse(xml.as_bytes())?;
        Some(show(&doc, doc.root))
    }

    /// What QXmlStreamReader gives Document::load: comments and instructions leave no node
    /// but end a text, references are part of their text, CDATA is a node of its own.
    #[test]
    fn a_document_is_read_into_its_nodes() {
        assert_eq!(
            tree("<?xml version=\"1.0\"?>\n<!-- c --><r a=\"1\" b='x&amp;y'>t<k>&lt;&#65;</k><![CDATA[c<]]> <e/>x<!-- c -->y</r>\n").as_deref(),
            Some("(r a=\"1\" b=\"x&y\" \"t\" (k \"<A\") [c<] \" \" (e) \"x\" \"y\")")
        );
        assert_eq!(
            tree("<!DOCTYPE a [<!ENTITY e \"x&f;\"><!ENTITY f \"y\">]><a>&e;</a>").as_deref(),
            Some("(a \"xy\")")
        );
        assert_eq!(
            tree("<p:a xmlns:p=\"u\" p:k=\"v\"/>").as_deref(),
            Some("(a k=\"v\")")
        );
    }

    /// `body` after a document type with the internal subset `subset`.
    fn typed(subset: &str, body: &str) -> Option<String> {
        tree(&format!("<!DOCTYPE a [{subset}]>{body}"))
    }

    /// Entities of 8, 64, 512 and 4096 characters, `a` to `d`.
    const POWERS: &str = "<!ENTITY a 'vvvvvvvv'><!ENTITY b '&a;&a;&a;&a;&a;&a;&a;&a;'>\
        <!ENTITY c '&b;&b;&b;&b;&b;&b;&b;&b;'><!ENTITY d '&c;&c;&c;&c;&c;&c;&c;&c;'>";

    /// What Qt 6.12.0 makes of the entities a document type declares.
    #[test]
    fn a_document_type_declares_entities() {
        for (subset, body, want) in [
            // The first declaration of a name counts; the five of XML are declared before.
            ("<!ENTITY e '1'><!ENTITY e '2'>", "<a>&e;</a>", "(a \"1\")"),
            ("<!ENTITY amp 'zz'>", "<a>&amp;</a>", "(a \"&\")"),
            // A value is read as markup again; in an attribute it is text.
            ("<!ENTITY e '<b>t</b>'>", "<a>&e;</a>", "(a (b \"t\"))"),
            ("<!ENTITY e '<b/>'>", "<a k='x&e;y'/>", "(a k=\"x<b/>y\")"),
            ("<!ENTITY e 'v&lt;'>", "<a k='x&e;y'/>", "(a k=\"xv<y\")"),
            (
                "<!ENTITY e '&#60;b/>'><!ENTITY f '&#38;#60;'>",
                "<a>&e;|&f;</a>",
                "(a (b) \"|<\")",
            ),
            // A value nobody refers to is not looked at.
            (
                "<!ENTITY e 'x&u;y'><!ENTITY f '&f;'>",
                "<a>z</a>",
                "(a \"z\")",
            ),
            // An external entity has no text; one with a notation only has to stay unused.
            ("<!ENTITY e SYSTEM 's'>", "<a>x&e;y</a>", "(a \"xy\")"),
            (
                "<!ENTITY e PUBLIC '-//p' 's'><!ENTITY e 'v'>",
                "<a>x&e;y</a>",
                "(a \"xy\")",
            ),
            (
                "<!NOTATION n SYSTEM 'n'><!ENTITY e SYSTEM 's' NDATA n>",
                "<a>x</a>",
                "(a \"x\")",
            ),
            // A parameter entity is more of the subset; its names are apart from the others.
            (
                "<!ENTITY % p \"<!ENTITY e 'v'>\">%p;",
                "<a>x&e;y</a>",
                "(a \"xvy\")",
            ),
            (
                "<!ENTITY % p \"<!ENTITY e 'ab\">%p;cd'>",
                "<a>&e;</a>",
                "(a \"abcd\")",
            ),
            ("<!ENTITY p 'g'><!ENTITY % p ''>", "<a>&p;</a>", "(a \"g\")"),
            ("<!ENTITY % p 'v'>", "<a>x%p;y</a>", "(a \"x%p;y\")"),
            // After a reference to one, an entity nobody declared is passed over and ends
            // the text; in an attribute it stays as written.
            ("<!ENTITY % p ''>%p;", "<a>x&u;y</a>", "(a \"x\" \"y\")"),
            ("<!ENTITY % p ''>%p;", "<a k='x&u;y'/>", "(a k=\"x&u;y\")"),
            // After an external one, declarations do not count; those before it do.
            (
                "<!ENTITY % p SYSTEM 's'>%p;<!ENTITY e 'v'>",
                "<a>x&e;y</a>",
                "(a \"x\" \"y\")",
            ),
            (
                "<!ENTITY e 'v'><!ENTITY % p SYSTEM 's'>%p;",
                "<a>x&e;y</a>",
                "(a \"xvy\")",
            ),
            // The other declarations only have to be well formed.
            (
                "<!ELEMENT a (#PCDATA|b)*><!ELEMENT b EMPTY><!-- c --><?pi x?>",
                "<a>x</a>",
                "(a \"x\")",
            ),
            (
                "<!ELEMENT a ((b|c)*,(d,e)+)?><!NOTATION n PUBLIC 'p'>",
                "<a>x</a>",
                "(a \"x\")",
            ),
        ] {
            assert_eq!(
                typed(subset, body).as_deref(),
                Some(want),
                "{subset} {body}"
            );
        }
        for (subset, body) in [
            ("<!ENTITY e SYSTEM 's'>", "<a k='&e;'/>"),
            ("<!ENTITY e SYSTEM 's' NDATA n>", "<a>&e;</a>"),
            ("<!ENTITY % p SYSTEM 's' NDATA n>", "<a/>"),
            ("<!ENTITY e 'x&u;y'>", "<a>&e;</a>"),
            ("<!ENTITY e 'x&e;y'>", "<a>&e;</a>"),
            ("<!ENTITY e '<b>'>", "<a>&e;</b></a>"),
            ("<!ENTITY e '<p:b/>'>", "<a xmlns:p='u'>&e;</a>"),
            ("<!ENTITY e '100%'>", "<a/>"),
            ("<!ENTITY % p \"<!ENTITY e 'v'>\">", "<a>&e;</a>"),
            ("<!ENTITY % p '&#37;p;'>%p;", "<a/>"),
            ("<!ENTITY % p \"<!ENTITY e 'v'\">%p;>", "<a/>"),
            ("<!ENTITY %p 'w'>", "<a/>"),
            ("%p;", "<a/>"),
            ("<!FOO a>", "<a/>"),
            ("<!ENTITY e>", "<a/>"),
            ("<!ENTITY e v>", "<a/>"),
            (" junk ", "<a/>"),
            ("<![INCLUDE[<!ENTITY e 'v'>]]>", "<a/>"),
            ("<!ELEMENT a (b,c|d)>", "<a/>"),
            ("<!NOTATION n>", "<a/>"),
        ] {
            assert_eq!(typed(subset, body), None, "{subset} {body}");
        }
        // The name of the document type is not looked at, what follows it is.
        assert_eq!(
            tree("<!DOCTYPE b [<!ENTITY e 'v'>]><!-- c --><?pi x?> <a>&e;</a>").as_deref(),
            Some("(a \"v\")")
        );
        // An external subset may declare what the document refers to, unless it is standalone.
        assert_eq!(
            tree("<!DOCTYPE a PUBLIC '-//p' 'a.dtd' [<!ENTITY e 'v'>]><a>x&u;y&e;</a>").as_deref(),
            Some("(a \"x\" \"yv\")")
        );
        for xml in [
            "<?xml version='1.0' standalone='yes'?><!DOCTYPE a SYSTEM 'a.dtd'><a>x&u;y</a>",
            "<?xml version='1.0' standalone='yes'?><!DOCTYPE a [<!ENTITY % p ''>%p;]><a>&u;</a>",
            "<!DOCTYPE a PUBLIC 'p{' 'a.dtd'><a/>",
            "<!DOCTYPE a><!DOCTYPE a><a/>",
            "<a/><!DOCTYPE a>",
        ] {
            assert_eq!(tree(xml), None, "{xml}");
        }
    }

    /// One reference may expand to 4096 characters, the references inside it counted
    /// (entityExpansionLimit); the next one starts from nothing.
    #[test]
    fn an_entity_expands_to_4096_characters() {
        let text = |n: usize| Some(format!("(a {:?})", "v".repeat(n)));
        assert_eq!(typed(POWERS, "<a>&d;</a>"), text(4096));
        assert_eq!(typed(POWERS, "<a>&d;&d;</a>"), text(8192));
        let v = "v".repeat(4096);
        assert_eq!(
            typed(POWERS, "<a k='&d;'>&d;</a>"),
            Some(format!("(a k={v:?} {v:?})"))
        );
        for body in ["<a>&e;</a>", "<a k='&e;'/>", "<a>&f;</a>"] {
            let subset = format!("{POWERS}<!ENTITY e '&d;v'><!ENTITY f '&c;&d;'>");
            assert_eq!(typed(&subset, body), None, "{body}");
        }
    }

    /// What Qt 6.12.0 makes of the attributes a document type declares.
    #[test]
    fn a_document_type_declares_attributes() {
        for (subset, body, want) in [
            // A default comes after the written attributes; `""` is none.
            (
                "<!ATTLIST a d CDATA 'dv' k CDATA #IMPLIED>",
                "<a k='1'/>",
                "(a k=\"1\" d=\"dv\")",
            ),
            ("<!ATTLIST a d CDATA \"\">", "<a/>", "(a)"),
            (
                "<!ENTITY e ''><!ATTLIST a d CDATA '&e;'>",
                "<a/>",
                "(a d=\"\")",
            ),
            // What is no CDATA has single spaces, as a default and as written.
            (
                "<!ATTLIST a d NMTOKEN #FIXED ' dv  w '>",
                "<a/>",
                "(a d=\"dv w\")",
            ),
            (
                "<!ATTLIST b q (x|y) 'x'>",
                "<a><b/><b q='y'/></a>",
                "(a (b q=\"x\") (b q=\"y\"))",
            ),
            (
                "<!ATTLIST a k NMTOKENS #IMPLIED>",
                "<a k='  x   y  ' j='  x  '/>",
                "(a k=\"x y\" j=\"  x  \")",
            ),
            // The first declaration of an attribute counts.
            (
                "<!ATTLIST a d CDATA '1' d CDATA '2'><!ATTLIST a d CDATA '3' k ID '4'>",
                "<a/>",
                "(a d=\"1\" k=\"4\")",
            ),
            // A default is as written before the first tag: its line ends stay, and a
            // carriage return with its line feed is a carriage return.
            (
                "<!ATTLIST a d CDATA 'x\n\ty' n NMTOKENS ' x\n\ty '>",
                "<a/>",
                "(a d=\"x\\n\\ty\" n=\"x y\")",
            ),
            (
                "<!ATTLIST a d CDATA 'x\ry\r\nz'>",
                "<a/>",
                "(a d=\"x\\ry\\rz\")",
            ),
            // None after a reference to an external parameter entity.
            (
                "<!ENTITY % p SYSTEM 's'>%p;<!ATTLIST a d CDATA 'dv'>",
                "<a/>",
                "(a)",
            ),
        ] {
            assert_eq!(
                typed(subset, body).as_deref(),
                Some(want),
                "{subset} {body}"
            );
        }
        for (subset, body) in [
            ("<!ATTLIST a d CDATA#IMPLIED>", "<a/>"),
            ("<!ATTLIST a d CDATA 'x&u;y'>", "<a/>"),
            ("<!ATTLIST a p:d CDATA 'dv'>", "<a/>"),
            (
                "<!ATTLIST a q:d CDATA 'dv'>",
                "<a xmlns:p='u' xmlns:q='u' p:d='w'/>",
            ),
        ] {
            assert_eq!(typed(subset, body), None, "{subset} {body}");
        }
        // A default `xmlns` declares a namespace, over the one the tag writes.
        let namespaces = |xml: &str| {
            let doc = parse(xml.as_bytes()).unwrap();
            let of = |n: &Node| format!("{}={}", n.name, n.namespace);
            doc.nodes.iter().map(of).collect::<Vec<_>>().join(" ")
        };
        assert_eq!(
            namespaces(
                "<!DOCTYPE a [<!ATTLIST a xmlns CDATA 'd' xmlns:p CDATA 'p'>]><a xmlns='w'><p:b/></a>"
            ),
            "a=d b=p"
        );
        assert_eq!(
            namespaces("<!DOCTYPE a [<!ATTLIST a p:d CDATA 'dv'>]><a xmlns:p='u'/>"),
            "a= d=u"
        );
    }

    /// A name of an element, of its prefix and of the target of an instruction with data is
    /// one of XML 1.0, fourth edition, as Qt 6.12.0 has it; an attribute's is anything.
    #[test]
    fn names_are_those_of_qt() {
        for xml in [
            "<\u{e9}/>",
            "<\u{4e2d}/>",
            "<a\u{b7}/>",
            "<a\u{301}/>",
            "<a\u{660}/>",
            "<a \u{2014}='1'/>",
            "<a k\u{1F600}='1'/>",
            "<a><?p\u{b7} x?></a>",
            "<a><?p\u{2014}?></a>",
            "<!DOCTYPE a [<!ATTLIST a d (x\u{301}|\u{e9}) #IMPLIED>]><a/>",
        ] {
            assert!(tree(xml).is_some(), "{xml}");
        }
        for xml in [
            "<a\u{2014}/>",
            "<\u{b7}a/>",
            "<\u{301}a/>",
            "<\u{660}/>",
            "<a\u{d7}/>",
            "<a\u{1F600}/>",
            "<a><?\u{2014} x?></a>",
            "<p\u{2014}:a xmlns:p\u{2014}='u'/>",
            "<!DOCTYPE a [<!ATTLIST a d (x\u{24b6}) #IMPLIED>]><a/>",
            "<!DOCTYPE a [<!ATTLIST a d (\u{1F600}) #IMPLIED>]><a/>",
        ] {
            assert_eq!(tree(xml), None, "{xml}");
        }
    }

    /// The XML declaration as Qt 6.12.0 reads it: the root's namespace, the encoding and
    /// whether the document is standalone.
    #[test]
    fn the_declaration_is_read_as_a_tag() {
        let read = |attributes: &str, body: &str| {
            let xml = format!("<?xml version='1.0'{attributes}?>{body}");
            let doc = parse(xml.as_bytes())?;
            let namespace = doc.nodes[doc.root].namespace.clone();
            Some(format!("{namespace} {} {}", doc.encoding, doc.standalone))
        };
        for (attributes, body, want) in [
            // Of an encoding and of standalone the last one counts.
            (" encoding='utf-8' encoding='UTF-8'", "<a/>", " UTF-8 false"),
            (" standalone='yes' standalone='no'", "<a/>", "  false"),
            (
                " encoding='UTF-8' standalone='no' standalone='yes'",
                "<a/>",
                " UTF-8 true",
            ),
            // A value is an attribute's: its references are read.
            (" standalone='y&#101;s'", "<a/>", "  true"),
            (" encoding='UTF&#45;8'", "<a/>", " UTF-8 false"),
            // A namespace it declares is the whole document's.
            (" xmlns='u'", "<a/>", "u  false"),
            (" xmlns='u' xmlns='v'", "<a/>", "v  false"),
            (" xmlns:p='u' encoding='UTF-8'", "<p:a/>", "u UTF-8 false"),
            (" xmlns='u'", "<a xmlns=''/>", "  false"),
        ] {
            assert_eq!(
                read(attributes, body).as_deref(),
                Some(want),
                "{attributes}"
            );
        }
        for attributes in [
            " standalone='yes' encoding='UTF-8'",
            " encoding='foo-bar' encoding='UTF-8'",
            " encoding=''",
            " encoding='1'",
            " encoding='UTF-16'",
            " standalone='maybe'",
            " version='1.0'",
            " k='1'",
            " xmlns:p=''",
            " xml:lang='en'",
        ] {
            assert_eq!(read(attributes, "<a/>"), None, "{attributes}");
        }
    }

    /// The encoding is the one the first bytes show, then the one the declaration names.
    #[test]
    fn the_encoding_is_shown_or_named() {
        let utf16 = |xml: &str| {
            xml.encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        };
        let utf32 = |xml: &str| -> Vec<u8> {
            xml.chars()
                .flat_map(|c| u32::from(c).to_be_bytes())
                .collect()
        };
        let read = |data: &[u8]| parse(data).map(|doc| doc.nodes[doc.root + 1].data.clone());
        let named = |name: &str| format!("<?xml version='1.0' encoding='{name}'?><a>\u{e9}</a>");
        assert_eq!(read(&utf16("<a>\u{e9}</a>")).as_deref(), Some("\u{e9}"));
        assert_eq!(read(&utf32("<a>\u{e9}</a>")).as_deref(), Some("\u{e9}"));
        assert_eq!(read(&utf16(&named("UTF-16"))).as_deref(), Some("\u{e9}"));
        let latin1 = |xml: &str| xml.chars().map(|c| c as u8).collect::<Vec<_>>();
        assert_eq!(read(&latin1(&named("latin1"))).as_deref(), Some("\u{e9}"));
        // Of two names the last one decodes.
        let both = |first: &str, last: &str| {
            let xml =
                format!("<?xml version='1.0' encoding='{first}' encoding='{last}'?><a>\u{e9}</a>");
            read(&latin1(&xml))
        };
        assert_eq!(both("UTF-8", "latin1").as_deref(), Some("\u{e9}"));
        assert_eq!(both("latin1", "UTF-8"), None);
        // What is named has to be what the bytes are.
        assert_eq!(read(&latin1(&named("US-ASCII"))), None);
        assert_eq!(read(&latin1(&named("UTF-8"))), None);
        assert_eq!(
            read(&[&[0xFF, 0xFE][..], &utf16(&named("UTF-8"))].concat()),
            None
        );
    }

    /// `n` entity values, each declared by a document type inside the one before it and
    /// referred to from an attribute there; the last one is `innermost`.
    fn nested(n: usize, innermost: &str) -> String {
        let mut inner = innermost.to_string();
        for level in (1..=n).rev() {
            let (quote, other, code) = match level % 2 {
                1 => ('"', '\'', "&#34;"),
                _ => ('\'', '"', "&#39;"),
            };
            let value = inner.replace('&', "&#38;").replace(quote, code);
            let (root, first) = match level {
                1 => ("a", "<!ENTITY % p \"\">%p;"),
                _ => ("x", ""),
            };
            inner = format!(
                "<!DOCTYPE {root} [{first}<!ENTITY e{level} {quote}{value}{quote}>]>\
                 <{root} k={other}&e{level};{other}/>"
            );
        }
        inner
    }

    /// QXmlStreamReader checks the value of an entity with a reader of its own, which may
    /// find a document type that declares the next: however deep, as long as each value
    /// fits the 4096 characters.
    #[test]
    fn entity_values_are_checked_however_deep() {
        assert_eq!(tree(&nested(1, "v")).as_deref(), Some("(a k=\"v\")"));
        assert!(tree(&nested(18, "<b/>")).is_some());
        assert_eq!(tree(&nested(20, "v")), None);
        // An error in the innermost value is an error of the document.
        assert_eq!(tree(&nested(18, "<b>")), None);
    }

    /// What QXmlStreamReader reports as an error is no document.
    #[test]
    fn a_broken_document_is_none() {
        for xml in [
            "",
            "x<a/>",
            "<a/>x",
            "<a/><b/>",
            "<a><b></a>",
            "<a>",
            "<a>&nbsp;</a>",
            "<a>a & b</a>",
            "<a k=\"1\" k=\"2\"/>",
            "<a k=\"1\"j=\"2\"/>",
            "<a k=\"<\"/>",
            "<p:a/>",
            " <?xml version=\"1.0\"?><a/>",
            "<?xml version=\"1.1\"?><a/>",
            "<?xml encoding=\"UTF-8\"?><a/>",
            "<?xml version=\"1.0\" encoding=\"foo-bar\"?><a/>",
            "<!DOCTYPE a [<!ENTITY e \"&e;\">]><a>&e;</a>",
            "<a><!-- a -- b --></a>",
            "<a>x]]>y</a>",
            "<a>x\u{1}y</a>",
            "<a>&#1;</a>",
            "<a xmlns:p=\"\"/>",
            "<a xmlns:p=\"u\" xmlns:q=\"u\" p:k=\"1\" q:k=\"2\"/>",
            "<a><?pi+x?></a>",
        ] {
            assert_eq!(tree(xml), None, "{xml}");
        }
        assert!(parse(b"<a>\xe9</a>").is_none());
    }
}
