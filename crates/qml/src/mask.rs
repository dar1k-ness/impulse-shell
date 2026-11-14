//! TextInput.inputMask (QQuickTextInputPrivate::parseInputMask, maskString, clearString,
//! stripString, isValidInput, findInMask, hasAcceptableInput).

#[derive(Clone, Copy, PartialEq)]
enum Case {
    None,
    Upper,
    Lower,
}

#[derive(Clone, Copy)]
struct Slot {
    ch: char,
    sep: bool,
    case: Case,
}

pub struct Mask {
    slots: Vec<Slot>,
    pub blank: char,
}

impl Mask {
    /// `None` for an empty mask or one starting with `;` (no mask).
    pub fn parse(fields: &str) -> Option<Mask> {
        let (mask, blank) = match fields.find(';') {
            Some(0) => return None,
            Some(d) => (&fields[..d], fields[d + 1..].chars().next().unwrap_or(' ')),
            None if fields.is_empty() => return None,
            None => (fields, ' '),
        };
        let mut slots = Vec::new();
        let (mut case, mut escape) = (Case::None, false);
        for c in mask.chars() {
            if escape {
                slots.push(Slot {
                    ch: c,
                    sep: true,
                    case,
                });
                escape = false;
                continue;
            }
            match c {
                '<' => case = Case::Lower,
                '>' => case = Case::Upper,
                '!' => case = Case::None,
                '{' | '}' | '[' | ']' => {}
                '\\' => escape = true,
                'A' | 'a' | 'N' | 'n' | 'X' | 'x' | '9' | '0' | 'D' | 'd' | '#' | 'H' | 'h'
                | 'B' | 'b' => slots.push(Slot {
                    ch: c,
                    sep: false,
                    case,
                }),
                _ => slots.push(Slot {
                    ch: c,
                    sep: true,
                    case,
                }),
            }
        }
        Some(Mask { slots, blank })
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn valid(&self, key: char, mask: char) -> bool {
        let b = key == self.blank;
        let num = key.is_numeric();
        let hex = num || key.is_ascii_hexdigit();
        let nz = num && key.to_digit(10).is_none_or(|d| d > 0);
        match mask {
            'A' => key.is_alphabetic(),
            'a' => key.is_alphabetic() || b,
            'N' => key.is_alphanumeric(),
            'n' => key.is_alphanumeric() || b,
            'X' => !key.is_control() && !b,
            'x' => !key.is_control() || b,
            '9' => num,
            '0' => num || b,
            'D' => nz,
            'd' => nz || b,
            '#' => num || key == '+' || key == '-' || b,
            'B' => key == '0' || key == '1',
            'b' => key == '0' || key == '1' || b,
            'H' => hex,
            'h' => hex || b,
            _ => false,
        }
    }

    fn cased(c: char, case: Case) -> char {
        match case {
            Case::Upper => c.to_uppercase().next().unwrap_or(c),
            Case::Lower => c.to_lowercase().next().unwrap_or(c),
            Case::None => c,
        }
    }

    pub fn find(
        &self,
        pos: usize,
        forward: bool,
        sep: bool,
        search: Option<char>,
    ) -> Option<usize> {
        let mut i = pos as isize;
        let n = self.len() as isize;
        if i >= n || i < 0 {
            return None;
        }
        while (0..n).contains(&i) {
            let s = self.slots[i as usize];
            let hit = if sep {
                s.sep && Some(s.ch) == search
            } else {
                !s.sep && search.is_none_or(|c| self.valid(c, s.ch))
            };
            if hit {
                return Some(i as usize);
            }
            i += if forward { 1 } else { -1 };
        }
        None
    }

    pub fn next_blank(&self, pos: usize) -> usize {
        self.find(pos, true, false, None).unwrap_or(self.len())
    }

    pub fn prev_blank(&self, pos: usize) -> usize {
        self.find(pos, false, false, None).unwrap_or(0)
    }

    pub fn clear(&self, pos: usize, len: usize) -> Vec<char> {
        let end = self.len().min(pos + len);
        (pos.min(end)..end)
            .map(|i| {
                if self.slots[i].sep {
                    self.slots[i].ch
                } else {
                    self.blank
                }
            })
            .collect()
    }

    /// maskString(pos, str, clear) against the current display text `text`.
    pub fn mask_string(&self, pos: usize, s: &[char], clear: bool, text: &[char]) -> Vec<char> {
        let n = self.len();
        if pos >= n {
            return Vec::new();
        }
        let fill = if clear {
            self.clear(0, n)
        } else {
            text.to_vec()
        };
        let at = |k: usize| fill.get(k).copied().unwrap_or(self.blank);
        let (mut out, mut si, mut i) = (Vec::new(), 0, pos);
        while i < n && si < s.len() {
            let slot = self.slots[i];
            let c = s[si];
            if slot.sep {
                out.push(slot.ch);
                if c == slot.ch {
                    si += 1;
                }
                i += 1;
                continue;
            }
            if self.valid(c, slot.ch) {
                out.push(Self::cased(c, slot.case));
                i += 1;
            } else if let Some(f) = self.find(i, true, true, Some(c)) {
                if s.len() != 1 || i == 0 || !self.slots[i - 1].sep || self.slots[i - 1].ch != c {
                    out.extend((i..=f).map(at));
                    i = f + 1;
                }
            } else if let Some(f) = self.find(i, true, false, Some(c)) {
                out.extend((i..f).map(at));
                out.push(Self::cased(c, self.slots[f].case));
                i = f + 1;
            }
            si += 1;
        }
        out
    }

    /// internalSetText: the display text (blanks shown) for `txt`.
    pub fn display(&self, txt: &str) -> Vec<char> {
        let s: Vec<char> = txt.chars().collect();
        let mut d = self.mask_string(0, &s, true, &[]);
        let k = d.len();
        d.extend(self.clear(k, self.len() - k.min(self.len())));
        d
    }

    /// stripString: what `text` reads.
    pub fn strip(&self, d: &[char]) -> String {
        let end = self.len().min(d.len());
        (0..end)
            .filter_map(|i| {
                if self.slots[i].sep {
                    Some(self.slots[i].ch)
                } else {
                    (d[i] != self.blank).then_some(d[i])
                }
            })
            .collect()
    }

    /// hasAcceptableInput's mask part: every slot holds a valid character.
    pub fn acceptable(&self, d: &[char]) -> bool {
        d.len() == self.len()
            && self.slots.iter().zip(d).all(|(s, &c)| {
                if s.sep {
                    c == s.ch
                } else {
                    self.valid(c, s.ch)
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[char]) -> String {
        v.iter().collect()
    }

    #[test]
    fn masks_format_filter_and_strip_like_qt() {
        let m = Mask::parse("99-99;_").unwrap();
        assert_eq!(s(&m.display("")), "__-__");
        assert_eq!(m.strip(&m.display("")), "-");
        assert_eq!(s(&m.display("1234")), "12-34");
        assert_eq!(s(&m.display("1a2")), "12-__");
        assert_eq!(s(&m.display("12-3")), "12-3_");
        assert!(m.acceptable(&m.display("1234")));
        assert!(!m.acceptable(&m.display("123")));
        let d = m.display("");
        assert_eq!(s(&m.mask_string(0, &['7'], false, &d)), "7");
        assert_eq!(s(&m.mask_string(2, &['5'], false, &d)), "-5");
        let up = Mask::parse(">AA<aa").unwrap();
        assert_eq!(s(&up.display("abCD")), "ABcd");
        let opt = Mask::parse("0000").unwrap();
        assert!(opt.acceptable(&opt.display("")));
        assert_eq!(opt.strip(&opt.display("")), "");
        assert!(Mask::parse(";_").is_none());
        let esc = Mask::parse("\\A9").unwrap();
        assert_eq!(s(&esc.display("5")), "A5");
    }
}
