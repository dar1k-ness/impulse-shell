//! The Unicode data HarfBuzz shapes with: the engine's (`icu_properties`, `icu_normalizer`,
//! the Unicode version of Qt 6.12) and not the tables built into HarfBuzz, which are of the
//! Unicode version HarfBuzz was released with. QTextEngine does the same: it sets Qt's
//! functions (hb_qt_get_unicode_funcs, qharfbuzzng.cpp) on every buffer it shapes.

use harfbuzz_sys as hb;
use icu_normalizer::properties::{
    CanonicalCompositionBorrowed, CanonicalDecompositionBorrowed, Decomposed,
};
use std::ffi::c_void;

pub(crate) fn general_category(c: char) -> hb::hb_unicode_general_category_t {
    use icu_properties::props::GeneralCategory as G;
    match icu_properties::CodePointMapData::<G>::new().get(c) {
        G::Control => hb::HB_UNICODE_GENERAL_CATEGORY_CONTROL,
        G::Format => hb::HB_UNICODE_GENERAL_CATEGORY_FORMAT,
        G::Unassigned => hb::HB_UNICODE_GENERAL_CATEGORY_UNASSIGNED,
        G::PrivateUse => hb::HB_UNICODE_GENERAL_CATEGORY_PRIVATE_USE,
        G::Surrogate => hb::HB_UNICODE_GENERAL_CATEGORY_SURROGATE,
        G::LowercaseLetter => hb::HB_UNICODE_GENERAL_CATEGORY_LOWERCASE_LETTER,
        G::ModifierLetter => hb::HB_UNICODE_GENERAL_CATEGORY_MODIFIER_LETTER,
        G::OtherLetter => hb::HB_UNICODE_GENERAL_CATEGORY_OTHER_LETTER,
        G::TitlecaseLetter => hb::HB_UNICODE_GENERAL_CATEGORY_TITLECASE_LETTER,
        G::UppercaseLetter => hb::HB_UNICODE_GENERAL_CATEGORY_UPPERCASE_LETTER,
        G::SpacingMark => hb::HB_UNICODE_GENERAL_CATEGORY_SPACING_MARK,
        G::EnclosingMark => hb::HB_UNICODE_GENERAL_CATEGORY_ENCLOSING_MARK,
        G::NonspacingMark => hb::HB_UNICODE_GENERAL_CATEGORY_NON_SPACING_MARK,
        G::DecimalNumber => hb::HB_UNICODE_GENERAL_CATEGORY_DECIMAL_NUMBER,
        G::LetterNumber => hb::HB_UNICODE_GENERAL_CATEGORY_LETTER_NUMBER,
        G::OtherNumber => hb::HB_UNICODE_GENERAL_CATEGORY_OTHER_NUMBER,
        G::ConnectorPunctuation => hb::HB_UNICODE_GENERAL_CATEGORY_CONNECT_PUNCTUATION,
        G::DashPunctuation => hb::HB_UNICODE_GENERAL_CATEGORY_DASH_PUNCTUATION,
        G::ClosePunctuation => hb::HB_UNICODE_GENERAL_CATEGORY_CLOSE_PUNCTUATION,
        G::FinalPunctuation => hb::HB_UNICODE_GENERAL_CATEGORY_FINAL_PUNCTUATION,
        G::InitialPunctuation => hb::HB_UNICODE_GENERAL_CATEGORY_INITIAL_PUNCTUATION,
        G::OtherPunctuation => hb::HB_UNICODE_GENERAL_CATEGORY_OTHER_PUNCTUATION,
        G::OpenPunctuation => hb::HB_UNICODE_GENERAL_CATEGORY_OPEN_PUNCTUATION,
        G::CurrencySymbol => hb::HB_UNICODE_GENERAL_CATEGORY_CURRENCY_SYMBOL,
        G::ModifierSymbol => hb::HB_UNICODE_GENERAL_CATEGORY_MODIFIER_SYMBOL,
        G::MathSymbol => hb::HB_UNICODE_GENERAL_CATEGORY_MATH_SYMBOL,
        G::OtherSymbol => hb::HB_UNICODE_GENERAL_CATEGORY_OTHER_SYMBOL,
        G::LineSeparator => hb::HB_UNICODE_GENERAL_CATEGORY_LINE_SEPARATOR,
        G::ParagraphSeparator => hb::HB_UNICODE_GENERAL_CATEGORY_PARAGRAPH_SEPARATOR,
        G::SpaceSeparator => hb::HB_UNICODE_GENERAL_CATEGORY_SPACE_SEPARATOR,
    }
}

fn combining_class(c: char) -> u8 {
    use icu_properties::props::CanonicalCombiningClass as C;
    icu_properties::CodePointMapData::<C>::new().get(c).0
}

/// The character that is `c` mirrored (QChar::mirroredChar), `c` where there is none.
fn mirroring(c: char) -> char {
    use icu_properties::props::BidiMirroringGlyph as M;
    let mirrored = icu_properties::CodePointMapData::<M>::new().get(c);
    mirrored.mirroring_glyph.unwrap_or(c)
}

/// The Script property of `c` under HarfBuzz's name for it.
pub(crate) fn script(c: char) -> hb::hb_script_t {
    use icu_properties::props::Script;
    let script = icu_properties::CodePointMapData::<Script>::new().get(c);
    let name = icu_properties::PropertyNamesShort::<Script>::new().get(script);
    let tag = name.map_or(*b"Zzzz", |n| n.as_bytes().try_into().unwrap_or(*b"Zzzz"));
    // SAFETY: a pure function of the four letters.
    unsafe { hb::hb_script_from_iso15924_tag(u32::from_be_bytes(tag)) }
}

/// The character a pair of the composition table gives, as QChar's ligatureHelper finds it.
/// Qt's table holds, for the second character, the firsts in units of UTF-16 of the second's
/// kind (one for the BMP, two past it), so a pair of a character past the BMP and one in it is
/// never found: U+105D2 and U+0307 are not U+105C9.
fn ligature(a: u32, b: u32) -> Option<char> {
    let (a, b) = (char::from_u32(a)?, char::from_u32(b)?);
    if a.len_utf16() != b.len_utf16() {
        return None;
    }
    CanonicalCompositionBorrowed::new().compose(a, b)
}

/// The first character of the NFC of `text` as QString::normalized gives it, and whether it
/// is all of it. The decomposition and the order of the marks are the standard's; the
/// composition is QChar's composeHelper, unit of UTF-16 by unit as it is written, because
/// HarfBuzz gets its answers and they show: once it has put a composite past the BMP in
/// place of the starter it steps over one unit more than it removed, the next character is
/// read from its second unit and composes with nothing. U+1611E U+1611E U+1611F stays
/// U+16121 U+1611F in Qt where the standard has U+16126 (docs/qt-differences.md), and a font
/// with a glyph for each draws the two.
fn nfc_as_qt(text: &str) -> (char, bool) {
    let nfd = icu_normalizer::DecomposingNormalizerBorrowed::new_nfd().normalize(text);
    let mut s: Vec<u16> = nfd.encode_utf16().collect();
    let (mut starter, mut starter_at, mut next, mut last_class) = (0, None, usize::MAX, 255);
    let mut pos = 0;
    while pos < s.len() {
        let at = pos;
        let mut c = u32::from(s[pos]);
        if let Some(Ok(pair)) = char::decode_utf16(s[pos..].iter().copied().take(2)).next()
            && pair.len_utf16() == 2
        {
            c = pair.into();
            pos += 1;
        }
        let class = char::from_u32(c).map_or(0, combining_class);
        if let Some(to) = starter_at.filter(|_| at == next || class > last_class)
            && let Some(composed) = ligature(starter, c)
        {
            starter = composed.into();
            let units = composed.encode_utf16(&mut s[to..to + 2]).len();
            s.drain(at..(at + units).min(s.len()));
            continue;
        }
        if class == 0 {
            (starter, starter_at, next) = (c, Some(at), pos + 1);
        }
        last_class = class;
        pos += 1;
    }
    let first = char::decode_utf16(s.iter().copied()).next();
    let first = first.and_then(Result::ok).unwrap_or('\0');
    (first, first.len_utf16() == s.len())
}

/// The character `a` and `b` compose to, as Qt's function finds it: the NFC of the pair where
/// it is one character (_hb_qt_unicode_compose). That is more than the primary composites of
/// the standard: "â" and U+0323 give "ậ", though the pair is not in canonical order.
fn compose(a: char, b: char) -> Option<char> {
    let mut pair = String::with_capacity(8);
    pair.push(a);
    pair.push(b);
    let (first, all) = nfc_as_qt(&pair);
    all.then_some(first)
}

/// The two characters `ab` decomposes to, or the one, as Qt's function finds them
/// (_hb_qt_unicode_decompose): the canonical decomposition of the standard, not recursive,
/// and where its first part decomposes again into characters that do not compose back
/// (a composition exclusion), the first character of the full decomposition alone. U+FB2C
/// is U+FB49 U+05C1, U+FB49 is U+05E9 U+05BC and excluded: Qt answers U+05E9, and a
/// character it decomposes this way loses the rest.
fn decompose(ab: char) -> Option<(char, Option<char>)> {
    match CanonicalDecompositionBorrowed::new().decompose(ab) {
        Decomposed::Default => None,
        Decomposed::Singleton(a) => Some((a, None)),
        Decomposed::Expansion(a, b) => {
            let mut pair = String::with_capacity(8);
            pair.push(a);
            pair.push(b);
            let (first, _) = nfc_as_qt(&pair);
            if first != a && first != ab {
                Some((first, None))
            } else {
                Some((a, Some(b)))
            }
        }
    }
}

unsafe extern "C" fn general_category_func(
    _: *mut hb::hb_unicode_funcs_t,
    c: hb::hb_codepoint_t,
    _: *mut c_void,
) -> hb::hb_unicode_general_category_t {
    char::from_u32(c).map_or(hb::HB_UNICODE_GENERAL_CATEGORY_SURROGATE, general_category)
}

unsafe extern "C" fn combining_class_func(
    _: *mut hb::hb_unicode_funcs_t,
    c: hb::hb_codepoint_t,
    _: *mut c_void,
) -> hb::hb_unicode_combining_class_t {
    char::from_u32(c).map_or(0, combining_class).into()
}

unsafe extern "C" fn mirroring_func(
    _: *mut hb::hb_unicode_funcs_t,
    c: hb::hb_codepoint_t,
    _: *mut c_void,
) -> hb::hb_codepoint_t {
    char::from_u32(c).map_or(c, |c| mirroring(c).into())
}

unsafe extern "C" fn script_func(
    _: *mut hb::hb_unicode_funcs_t,
    c: hb::hb_codepoint_t,
    _: *mut c_void,
) -> hb::hb_script_t {
    char::from_u32(c).map_or(hb::HB_SCRIPT_UNKNOWN, script)
}

unsafe extern "C" fn compose_func(
    _: *mut hb::hb_unicode_funcs_t,
    a: hb::hb_codepoint_t,
    b: hb::hb_codepoint_t,
    ab: *mut hb::hb_codepoint_t,
    _: *mut c_void,
) -> hb::hb_bool_t {
    let composed = match (char::from_u32(a), char::from_u32(b)) {
        (Some(a), Some(b)) => compose(a, b),
        _ => None,
    };
    // SAFETY: HarfBuzz passes a place for the answer.
    unsafe { *ab = composed.map_or(0, u32::from) };
    composed.is_some().into()
}

unsafe extern "C" fn decompose_func(
    _: *mut hb::hb_unicode_funcs_t,
    ab: hb::hb_codepoint_t,
    a: *mut hb::hb_codepoint_t,
    b: *mut hb::hb_codepoint_t,
    _: *mut c_void,
) -> hb::hb_bool_t {
    let parts = char::from_u32(ab).and_then(decompose);
    let (first, second) = parts.map_or((ab, 0), |(a, b)| (a.into(), b.map_or(0, u32::from)));
    // SAFETY: HarfBuzz passes a place for the first answer.
    unsafe { *a = first };
    // SAFETY: and one for the second.
    unsafe { *b = second };
    parts.is_some().into()
}

struct Funcs(*mut hb::hb_unicode_funcs_t);
// SAFETY: the object is immutable once made and HarfBuzz counts its references atomically.
unsafe impl Send for Funcs {}
// SAFETY: as above.
unsafe impl Sync for Funcs {}

/// The functions to set on a buffer (hb_buffer_set_unicode_funcs); made once, never freed.
pub(crate) fn funcs() -> *mut hb::hb_unicode_funcs_t {
    static FUNCS: std::sync::OnceLock<Funcs> = std::sync::OnceLock::new();
    FUNCS
        .get_or_init(|| {
            let (data, none) = (std::ptr::null_mut(), None);
            // SAFETY: a new object with no parent, as hb_qt_get_unicode_funcs makes it.
            let f = unsafe { hb::hb_unicode_funcs_create(std::ptr::null_mut()) };
            // SAFETY: `f` is the live, still mutable object made above; the functions keep no
            // data, so there is nothing to destroy. The same holds for the five after it.
            unsafe {
                hb::hb_unicode_funcs_set_combining_class_func(
                    f,
                    Some(combining_class_func),
                    data,
                    none,
                );
            }
            // SAFETY: as above.
            unsafe {
                hb::hb_unicode_funcs_set_general_category_func(
                    f,
                    Some(general_category_func),
                    data,
                    none,
                );
            }
            // SAFETY: as above.
            unsafe { hb::hb_unicode_funcs_set_mirroring_func(f, Some(mirroring_func), data, none) };
            // SAFETY: as above.
            unsafe { hb::hb_unicode_funcs_set_script_func(f, Some(script_func), data, none) };
            // SAFETY: as above.
            unsafe { hb::hb_unicode_funcs_set_compose_func(f, Some(compose_func), data, none) };
            // SAFETY: as above.
            unsafe { hb::hb_unicode_funcs_set_decompose_func(f, Some(decompose_func), data, none) };
            // SAFETY: `f` is live; from here on nothing changes it.
            unsafe { hb::hb_unicode_funcs_make_immutable(f) };
            Funcs(f)
        })
        .0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    /// What Qt 6.12 gives HarfBuzz (tests/qt_hb/probe.cpp).
    const QT: &str = include_str!("../tests/qt_hb/qt.txt");

    fn hex(s: &str) -> u32 {
        u32::from_str_radix(s, 16).unwrap()
    }

    /// The lines `name FIRST..LAST VALUE` as (first, last, value).
    fn runs(name: &str) -> Vec<(u32, u32, &'static str)> {
        QT.lines()
            .filter_map(|l| l.strip_prefix(name)?.strip_prefix(' '))
            .map(|l| {
                let (range, value) = l.split_once(' ').unwrap_or((l, ""));
                let (first, last) = range.split_once("..").unwrap();
                (hex(first), hex(last), value)
            })
            .collect()
    }

    /// The lines `name A B ...` as numbers.
    fn rows(name: &str) -> Vec<Vec<u32>> {
        QT.lines()
            .filter_map(|l| l.strip_prefix(name)?.strip_prefix(' '))
            .map(|l| l.split(' ').map(hex).collect())
            .collect()
    }

    fn chars() -> impl Iterator<Item = char> {
        (0..=0x10FFFF).filter_map(char::from_u32)
    }

    /// The functions as HarfBuzz calls them.
    fn ours() -> *mut hb::hb_unicode_funcs_t {
        funcs()
    }

    #[test]
    fn every_code_point_has_the_category_qt_gives_harfbuzz() {
        let mut qt = BTreeMap::new();
        for (first, last, value) in runs("category") {
            qt.extend((first..=last).map(|c| (c, value.parse::<u32>().unwrap())));
        }
        // SAFETY: `ours()` is a live functions object.
        let differ = chars()
            .filter(
                |&c| unsafe { hb::hb_unicode_general_category(ours(), c.into()) } != qt[&c.into()],
            )
            .count();
        assert_eq!((qt.len(), differ), (0x110000 - 0x800, 0));
    }

    #[test]
    fn every_code_point_has_the_combining_class_qt_gives_harfbuzz() {
        let mut qt = BTreeMap::new();
        for (first, last, value) in runs("combining") {
            qt.extend((first..=last).map(|c| (c, value.parse::<u32>().unwrap())));
        }
        let differ = chars()
            .filter(|&c| {
                // SAFETY: `ours()` is a live functions object.
                let class = unsafe { hb::hb_unicode_combining_class(ours(), c.into()) };
                class != qt.get(&c.into()).copied().unwrap_or(0)
            })
            .count();
        assert_eq!(differ, 0);
    }

    #[test]
    fn every_code_point_has_the_mirrored_character_qt_gives_harfbuzz() {
        let qt: BTreeMap<u32, u32> = rows("mirror").iter().map(|r| (r[0], r[1])).collect();
        let differ = chars()
            .filter(|&c| {
                // SAFETY: `ours()` is a live functions object.
                let mirrored = unsafe { hb::hb_unicode_mirroring(ours(), c.into()) };
                mirrored != qt.get(&c.into()).copied().unwrap_or(c.into())
            })
            .count();
        assert_eq!((qt.len(), differ), (428, 0));
    }

    /// Qt's table from QChar::Script to HarfBuzz's scripts (_qtscript_to_hbscript) has the
    /// four scripts of Unicode 17 in another order than QChar::Script: a character of Sidetic
    /// goes to HarfBuzz as Beria Erfe, of Tai Yo as Sidetic, of Tolong Siki as Tai Yo, of
    /// Beria Erfe as Tolong Siki. Ours go as what they are (docs/qt-differences.md).
    #[test]
    fn every_code_point_has_the_script_qt_gives_harfbuzz_but_four_scripts() {
        let mut qt = BTreeMap::new();
        for (first, last, value) in runs("script") {
            qt.extend((first..=last).map(|c| (c, value)));
        }
        let tag = |c: char| {
            // SAFETY: `ours()` is a live functions object.
            let script = unsafe { hb::hb_unicode_script(ours(), c.into()) };
            // SAFETY: a pure function of its argument.
            let tag = unsafe { hb::hb_script_to_iso15924_tag(script) };
            String::from_utf8(tag.to_be_bytes().to_vec()).unwrap()
        };
        let (mut differ, mut other) = (BTreeMap::new(), vec![]);
        for c in chars() {
            let (ours, qt) = (tag(c), qt[&u32::from(c)]);
            if ours == qt {
                continue;
            }
            match (ours.as_str(), qt) {
                ("Sidt", "Berf") | ("Tayo", "Sidt") | ("Tols", "Tayo") | ("Berf", "Tols") => {
                    *differ.entry(ours).or_insert(0) += 1
                }
                _ => other.push(format!("U+{:04X} {ours} Qt {qt}", u32::from(c))),
            }
        }
        assert!(
            other.is_empty(),
            "{} differ: {:?}",
            other.len(),
            &other[..other.len().min(20)]
        );
        let counts: Vec<(&str, i32)> = differ.iter().map(|(k, &v)| (k.as_str(), v)).collect();
        assert_eq!(
            counts,
            [("Berf", 50), ("Sidt", 26), ("Tayo", 55), ("Tols", 54)]
        );
    }

    /// A syllable of Hangul and its two parts by the arithmetic of the standard (3.12).
    fn hangul(ab: u32) -> Option<(u32, u32)> {
        let s = ab.checked_sub(0xAC00).filter(|&s| s < 11172)?;
        Some(match s % 28 {
            0 => (0x1100 + s / 588, 0x1161 + s % 588 / 28),
            t => (ab - t, 0x11A7 + t),
        })
    }

    #[test]
    fn every_code_point_decomposes_as_qt_s_function_does() {
        let qt: BTreeMap<u32, (u32, u32)> = rows("decompose")
            .iter()
            .map(|r| (r[0], (r[1], r[2])))
            .collect();
        let mut differ = vec![];
        for c in chars() {
            let (mut a, mut b) = (0, 0);
            // SAFETY: `ours()` is a live functions object, `a` and `b` places for the answers.
            let ours = (unsafe { hb::hb_unicode_decompose(ours(), c.into(), &mut a, &mut b) } != 0)
                .then_some((a, b));
            let qt = qt.get(&c.into()).copied().or(hangul(c.into()));
            if ours != qt {
                differ.push(format!("U+{:04X} {ours:X?} Qt {qt:X?}", u32::from(c)));
            }
        }
        assert!(
            differ.is_empty(),
            "{} differ: {:?}",
            differ.len(),
            &differ[..differ.len().min(20)]
        );
        assert_eq!(rows("hangul")[0], [0x11172, 0x11172]);
    }

    #[test]
    fn every_pair_composes_as_qt_s_function_does() {
        let mut firsts = vec![];
        for (first, last, _) in runs("first") {
            firsts.extend(first..=last);
        }
        let seconds: Vec<u32> = rows("second").iter().map(|r| r[0]).collect();
        let qt: BTreeMap<(u32, u32), u32> = rows("compose")
            .iter()
            .map(|r| ((r[0], r[1]), r[2]))
            .collect();
        let syllables: BTreeSet<(u32, u32)> = (0xAC00..0xAC00 + 11172).filter_map(hangul).collect();
        let (mut pairs, mut composed, mut differ) = (0, 0, vec![]);
        for &a in &firsts {
            for &b in &seconds {
                let mut ab = 0;
                // SAFETY: `ours()` is a live functions object, `ab` a place for the answer.
                let ours =
                    (unsafe { hb::hb_unicode_compose(ours(), a, b, &mut ab) } != 0).then_some(ab);
                let qt = match qt.get(&(a, b)) {
                    Some(&ab) => Some(ab),
                    None if syllables.contains(&(a, b)) => Some(
                        (0xAC00..0xAC00 + 11172)
                            .find(|&s| hangul(s) == Some((a, b)))
                            .unwrap(),
                    ),
                    None => None,
                };
                pairs += 1;
                composed += u32::from(ours.is_some());
                if ours != qt {
                    differ.push(format!("U+{a:04X} U+{b:04X} {ours:X?} Qt {qt:X?}"));
                }
            }
        }
        assert!(
            differ.is_empty(),
            "{} differ: {:?}",
            differ.len(),
            &differ[..differ.len().min(20)]
        );
        assert_eq!((pairs, composed), (14503 * 142, 1070 + 11172));
    }
}
