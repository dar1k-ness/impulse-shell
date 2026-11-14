//! Text editor positions. QQuickTextInput/QQuickTextEdit count `cursorPosition`,
//! `selectionStart`/`selectionEnd` and their methods' arguments in QString UTF-16 code units,
//! the units of ECMAScript string indices; the editing code walks `char`s and the
//! text-input-v3 protocol counts UTF-8 bytes. Every conversion between them is here.
//!
//! Qt keeps a position inside a surrogate pair (its selectedText then holds a lone surrogate);
//! the text here is UTF-8, so such a position maps to the start of its character and an edit
//! never splits a character in two.

/// The UTF-16 length of `s`: QString::size, JS `length`.
pub fn len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// The `char` index of UTF-16 position `u` in `cs`, clamped to `cs.len()`. Also the count of
/// leading chars that fit in `u` UTF-16 units.
pub fn to_char(cs: &[char], u: usize) -> usize {
    let mut n = 0;
    for (k, c) in cs.iter().enumerate() {
        n += c.len_utf16();
        if n > u {
            return k;
        }
    }
    cs.len()
}

/// The UTF-16 position of `char` index `c` in `cs`, clamped to the text.
pub fn to_utf16(cs: &[char], c: usize) -> usize {
    cs[..c.min(cs.len())].iter().map(|c| c.len_utf16()).sum()
}

/// The UTF-8 byte offset of UTF-16 position `u` in `s`, clamped to `s.len()`.
pub fn to_byte(s: &str, u: usize) -> usize {
    let mut n = 0;
    for (b, c) in s.char_indices() {
        n += c.len_utf16();
        if n > u {
            return b;
        }
    }
    s.len()
}

/// The leading chars of `s` within `u` UTF-16 units: QString::left(u) without splitting a pair.
pub fn left(s: &str, u: usize) -> &str {
    &s[..to_byte(s, u)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn astral_chars_are_two_units() {
        let s = "😀aé";
        let cs: Vec<char> = s.chars().collect();
        assert_eq!(len(s), 4);
        assert_eq!(
            (0..=5).map(|u| to_char(&cs, u)).collect::<Vec<_>>(),
            [0, 0, 1, 2, 3, 3]
        );
        assert_eq!(
            (0..=4).map(|c| to_utf16(&cs, c)).collect::<Vec<_>>(),
            [0, 2, 3, 4, 4]
        );
        assert_eq!(
            (0..=5).map(|u| to_byte(s, u)).collect::<Vec<_>>(),
            [0, 0, 4, 5, 7, 7]
        );
        assert_eq!((left(s, 1), left(s, 2), left(s, 9)), ("", "😀", s));
    }
}
