//! QLocale number formatting and parsing (Qt 6.12 qlocale.cpp: `QLocale::toString(double,
//! char, int)`, `toCurrencyString`, `toDouble`) over the CLDR data in `locale_data.rs`.

use crate::locale::{Locale, NUM};

// QLocale::NumberOption.
bits::bit_flags! {
const OMIT_GROUP_SEPARATOR: u32 = 0x01;
const REJECT_GROUP_SEPARATOR: u32 = 0x02;
const OMIT_LEADING_ZERO_IN_EXPONENT: u32 = 0x04;
const REJECT_LEADING_ZERO_IN_EXPONENT: u32 = 0x08;
const INCLUDE_TRAILING_ZEROES_AFTER_DOT: u32 = 0x10;
const REJECT_TRAILING_ZEROES_AFTER_DOT: u32 = 0x20;
}

/// QLocale::FloatingPointShortest.
pub const SHORTEST: i32 = -128;

/// QLocaleData::DoubleForm.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    Decimal,
    Exponent,
    SignificantDigits,
}

/// QLocaleData::PrecisionMode.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pm {
    DecimalDigits,
    SignificantDigits,
    ChopTrailingZeros,
}

/// One QLocale's number data.
#[derive(Clone, Debug)]
pub struct Numeric {
    pub decimal: &'static str,
    pub group: &'static str,
    pub minus: &'static str,
    pub plus: &'static str,
    pub exponential: &'static str,
    pub zero: char,
    /// Digit grouping: the leftmost group needs `first` digits, the rest `higher`, the
    /// rightmost has `least`.
    pub grouping: (usize, usize, usize),
    pub currency_symbol: &'static str,
    pub currency_iso: &'static str,
    currency_format: &'static str,
    currency_format_negative: &'static str,
    currency_digits: i32,
    options: u32,
    cyrillic: bool,
    /// QLocaleData::c(): its parser takes only ASCII (`NumericData::isC`).
    is_c: bool,
}

impl Locale {
    /// The number data of this locale (a `QLocale(name)` or `Qt.locale(name)`).
    pub fn numeric(self) -> Numeric {
        let f = &self.fields()[NUM..];
        let mut g = f[6].split(',').map(|n| n.parse().unwrap_or(3));
        Numeric {
            decimal: f[0],
            group: f[1],
            minus: f[2],
            plus: f[3],
            exponential: f[4],
            zero: f[5].chars().next().unwrap_or('0'),
            grouping: (
                g.next().unwrap_or(1),
                g.next().unwrap_or(3),
                g.next().unwrap_or(3),
            ),
            currency_symbol: f[7],
            currency_iso: f[8],
            currency_format: f[9],
            currency_format_negative: f[10],
            currency_digits: f[11].parse().unwrap_or(2),
            options: f[12].parse().unwrap_or(0),
            cyrillic: f[13] == "1",
            is_c: self == Locale::c(),
        }
    }
}

impl Numeric {
    /// The default `QLocale()`: the system locale's number data (QSystemLocale's lc_numeric),
    /// which never parses as the C locale.
    pub fn system() -> Numeric {
        Numeric {
            is_c: false,
            ..Locale::SYSTEM.numeric()
        }
    }

    /// QLocale::toString(double, char format, int precision): `f`, `e`, `g` (upper case
    /// for upper-case exponents and inf/nan), anything else as `f`.
    pub fn to_string(&self, d: f64, format: u8, precision: i32) -> String {
        let form = match format.to_ascii_lowercase() {
            b'e' => Form::Exponent,
            b'g' => Form::SignificantDigits,
            _ => Form::Decimal,
        };
        self.double_to_string(d, precision, form, format.is_ascii_uppercase())
    }

    /// QLocale::toString(int): the digits in the locale's groups. `n` is within ±2^53.
    pub fn integer_to_string(&self, n: i64) -> String {
        self.to_string(n as f64, b'f', 0)
    }

    /// QLocale::numberOptions().
    pub fn options(&self) -> u32 {
        self.options
    }

    /// This data under other QLocale::NumberOptions (QLocale::setNumberOptions).
    pub fn with_options(self, options: u32) -> Numeric {
        Numeric { options, ..self }
    }

    /// QLocale::decimalPoint(), groupSeparator(), negativeSign(), positiveSign(),
    /// exponential() and zeroDigit().
    pub fn symbols(&self) -> [String; 6] {
        [
            self.decimal.to_string(),
            self.group.to_string(),
            self.minus.to_string(),
            self.plus.to_string(),
            self.exponential.to_string(),
            self.zero.to_string(),
        ]
    }

    /// QLocale::currencySymbol(CurrencySymbol) and (CurrencyIsoCode).
    pub fn currency(&self) -> (&'static str, &'static str) {
        (self.currency_symbol, self.currency_iso)
    }

    /// QLocale::toCurrencyString(double, symbol) with the locale's currency digits;
    /// `None` is the locale's symbol, an empty symbol its ISO code.
    pub fn to_currency_string(&self, value: f64, symbol: Option<&str>) -> String {
        let (format, value) = if self.currency_format_negative.is_empty() || value >= 0.0 {
            (self.currency_format, value)
        } else {
            (self.currency_format_negative, -value)
        };
        let num = self.to_string(value, b'f', self.currency_digits);
        let sym = match symbol.unwrap_or(self.currency_symbol) {
            "" => self.currency_iso,
            s => s,
        };
        // QString::arg(num, sym).
        let mut out = String::new();
        let mut rest = format;
        while let Some(i) = rest.find('%') {
            out.push_str(&rest[..i]);
            match rest.as_bytes().get(i + 1) {
                Some(b'1') => out.push_str(&num),
                Some(b'2') => out.push_str(sym),
                _ => {
                    out.push('%');
                    rest = &rest[i + 1..];
                    continue;
                }
            }
            rest = &rest[i + 2..];
        }
        out.push_str(rest);
        out
    }

    /// QLocale::toDouble: `None` where Qt reports `ok == false` (bad syntax, digit groups
    /// that do not match the locale's grouping, overflow, underflow to zero).
    pub fn to_double(&self, s: &str) -> Option<f64> {
        ascii_to_double(&self.number_to_c_locale(s)?)
    }

    fn digit(&self, n: u8) -> char {
        char::from_u32(self.zero as u32 + u32::from(n)).unwrap_or('0')
    }

    /// QLocaleData::longLongToString(n, -1, 10, width, ZeroPadded), the number in a field of
    /// a date or time: the locale's digits behind its negative sign, zeros in front up to
    /// `width`, which counts each digit and each UTF-16 unit of the sign.
    pub fn integer(&self, n: i64, width: usize) -> String {
        let digits = n.unsigned_abs().to_string();
        let sign = if n < 0 { self.minus } else { "" };
        let used = digits.len() + sign.encode_utf16().count();
        let mut out = sign.to_string();
        out.extend(std::iter::repeat_n(self.zero, width.saturating_sub(used)));
        out.extend(digits.bytes().map(|b| self.digit(b - b'0')));
        out
    }

    /// QLocaleData::digitSequence: the digits `text` starts with, at most `max`, each with
    /// where it ends in `text`. A digit is one of the locale's or an ASCII one, as Qt's
    /// tokenizer of numbers reads them.
    pub fn digit_sequence(&self, text: &[char], max: usize) -> Vec<(u8, usize)> {
        let mut tokens = Tokens {
            text,
            i: 0,
            n: self,
        };
        let mut out = Vec::new();
        while out.len() < max && !tokens.done() {
            match tokens.next() {
                t @ b'0'..=b'9' => out.push((t - b'0', tokens.i)),
                _ => break,
            }
        }
        out
    }

    /// The sign `text` starts with, as Qt's tokenizer of numbers reads one (the locale's or
    /// an ASCII one or U+2212, behind marks of direction): whether it is a minus, and where
    /// it ends in `text`.
    pub fn sign_at(&self, text: &[char]) -> Option<(bool, usize)> {
        let mut tokens = Tokens {
            text,
            i: 0,
            n: self,
        };
        if tokens.done() {
            return None;
        }
        match tokens.next() {
            b'-' => Some((true, tokens.i)),
            b'+' => Some((false, tokens.i)),
            _ => None,
        }
    }

    /// QLocaleData::doubleToString without width or padding flags.
    fn double_to_string(&self, d: f64, precision: i32, form: Form, capital: bool) -> String {
        let precision = if precision != SHORTEST && precision < 0 {
            6
        } else {
            precision
        };
        let group_digits = self.options & OMIT_GROUP_SEPARATOR == 0;
        let min_exponent_digits = if self.options & OMIT_LEADING_ZERO_IN_EXPONENT == 0 {
            2
        } else {
            1
        };
        let prefix = if d < 0.0 { self.minus } else { "" };
        let num = if d.is_nan() {
            "nan".to_string()
        } else if d.is_infinite() {
            "inf".to_string()
        } else {
            let (digits, decpt) = double_to_ascii(d.abs(), form, precision);
            let exponent = |digits, pm| {
                let mantissa = exponent_form(digits, precision, pm);
                self.render(&mantissa, Some((decpt - 1, min_exponent_digits)))
            };
            let decimal = |digits, pm| {
                let mantissa = decimal_form(
                    digits,
                    decpt,
                    precision,
                    pm,
                    group_digits.then_some(self.grouping),
                );
                self.render(&mantissa, None)
            };
            match form {
                Form::Exponent => exponent(digits, Pm::DecimalDigits),
                Form::Decimal => decimal(digits, Pm::DecimalDigits),
                Form::SignificantDigits => {
                    let pm = if self.options & INCLUDE_TRAILING_ZEROES_AFTER_DOT != 0 {
                        Pm::SignificantDigits
                    } else {
                        Pm::ChopTrailingZeros
                    };
                    // POSIX %g: decimal if P > X >= -4, else exponent (Qt's own rule for
                    // the shortest digits: whichever is shorter).
                    let use_decimal = if precision == SHORTEST {
                        let (first, higher, least) = self.grouping;
                        let decpt = decpt as i64;
                        let digit_count = digits.len() as i64;
                        let mut bias = 2 + min_exponent_digits as i64;
                        if group_digits && decpt >= (first + least) as i64 {
                            bias -= (decpt - least as i64) / higher as i64 + 1;
                        }
                        if decpt > 10 && min_exponent_digits == 1 {
                            bias += 1;
                        }
                        if digit_count <= decpt && digit_count > 1 {
                            bias += 1;
                        } else if digit_count == 1 && decpt <= 0 {
                            bias -= 1;
                        }
                        if decpt <= 0 {
                            1 - decpt <= bias
                        } else if decpt <= digit_count {
                            0 <= bias
                        } else {
                            decpt <= digit_count + bias
                        }
                    } else {
                        decpt > -4 && decpt <= precision.max(1)
                    };
                    if use_decimal {
                        decimal(digits, pm)
                    } else {
                        exponent(digits, pm)
                    }
                }
            }
        };
        let num = if capital {
            num.to_uppercase()
        } else {
            num.to_lowercase()
        };
        format!("{prefix}{num}")
    }

    /// Locale text for an ASCII layout of digits, `.` (decimal point) and `,` (group
    /// separator), plus an exponent (value, minimum digit count).
    fn render(&self, layout: &[u8], exponent: Option<(i32, usize)>) -> String {
        let mut out = String::new();
        for &b in layout {
            match b {
                b'.' => out.push_str(self.decimal),
                b',' => out.push_str(self.group),
                _ => out.push(self.digit(b - b'0')),
            }
        }
        if let Some((e, min_digits)) = exponent {
            out.push_str(self.exponential);
            out.push_str(if e < 0 { self.minus } else { self.plus });
            // QLocaleData::applyIntegerFormatting pads to `min_digits` UTF-16 units.
            let digits: Vec<char> = e
                .unsigned_abs()
                .to_string()
                .bytes()
                .map(|b| self.digit(b - b'0'))
                .collect();
            for _ in digits.len() * self.zero.len_utf16()..min_digits {
                out.push(self.zero);
            }
            out.extend(digits);
        }
        out
    }

    /// QLocaleData::numberToCLocale in DoubleScientificMode: the ASCII number for
    /// `qt_asciiToDouble`, group separators checked against the locale's grouping and dropped.
    fn number_to_c_locale(&self, s: &str) -> Option<Vec<u8>> {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        enum Stage {
            Whole,
            Grouped,
            Fraction,
            Exponent,
            Name,
        }
        let text: Vec<char> = s.trim_matches(char::is_whitespace).chars().collect();
        if text.is_empty() {
            return None;
        }
        let mut tokens = Tokens {
            text: &text,
            i: 0,
            n: self,
        };
        let (first, higher, least) = self.grouping;
        let opt = |o: u32| self.options & o != 0;
        let mut stage = Stage::Whole;
        let mut want_digits = true;
        let mut need_higher_group = false;
        let mut digits_in_group = 0;
        let bad_least_group = |stage: Stage, need_higher: bool, digits: usize| {
            stage == Stage::Grouped && (need_higher || digits != least)
        };
        let mut last = 0u8;
        let mut out = Vec::with_capacity(text.len());
        while !tokens.done() {
            let mut t = tokens.next();
            match t {
                0 => return None,
                b'.' => {
                    if stage > Stage::Grouped {
                        return None;
                    }
                    if tokens.fraction_group_clash()
                        && bad_least_group(stage, need_higher_group, digits_in_group)
                        && digits_in_group == higher
                    {
                        t = b',';
                        stage = Stage::Grouped;
                        need_higher_group = false;
                        digits_in_group = 0;
                    } else {
                        if bad_least_group(stage, need_higher_group, digits_in_group) {
                            return None;
                        }
                        stage = Stage::Fraction;
                    }
                }
                b'e' => {
                    if want_digits || stage == Stage::Name || stage > Stage::Fraction {
                        return None;
                    }
                    if stage < Stage::Fraction {
                        if bad_least_group(stage, need_higher_group, digits_in_group) {
                            return None;
                        }
                    } else if opt(REJECT_TRAILING_ZEROES_AFTER_DOT) && last == b'0' {
                        return None;
                    }
                    stage = Stage::Exponent;
                    want_digits = true;
                }
                b',' => {
                    if opt(REJECT_GROUP_SEPARATOR) {
                        return None;
                    }
                    match stage {
                        Stage::Whole => {
                            if digits_in_group == 0 || digits_in_group > first.max(higher) {
                                return None;
                            }
                            need_higher_group = first > digits_in_group;
                            stage = Stage::Grouped;
                        }
                        Stage::Grouped => {
                            if digits_in_group != higher {
                                return None;
                            }
                            need_higher_group = false;
                        }
                        _ => return None,
                    }
                    digits_in_group = 0;
                }
                b'0'..=b'9' => {
                    if stage == Stage::Name {
                        return None;
                    }
                    if t == b'0'
                        && opt(REJECT_LEADING_ZERO_IN_EXPONENT)
                        && stage > Stage::Fraction
                        && !tokens.done()
                        && !last.is_ascii_digit()
                    {
                        return None;
                    }
                    want_digits = false;
                    digits_in_group += 1;
                }
                _ if stage == Stage::Whole && is_inf_nan_char(t) => {
                    if !want_digits {
                        return None;
                    }
                    want_digits = false;
                    stage = Stage::Name;
                }
                _ => {}
            }
            last = t;
            if t != b',' {
                out.push(t);
            }
        }
        if want_digits {
            return None;
        }
        if !opt(REJECT_GROUP_SEPARATOR)
            && stage < Stage::Fraction
            && bad_least_group(stage, need_higher_group, digits_in_group)
        {
            return None;
        }
        if opt(REJECT_TRAILING_ZEROES_AFTER_DOT) && stage == Stage::Fraction && last == b'0' {
            return None;
        }
        Some(out)
    }

    /// QLocaleData::NumericData::digitValue.
    fn digit_value(&self, c: char) -> Option<u8> {
        let c = c as u32;
        let zero = self.zero as u32;
        if !self.is_c && self.zero != '0' {
            // Must match qlocale_tools_p.h's unicodeForDigit().
            let base = if c == zero || self.zero != '\u{3007}' {
                Some(zero)
            } else {
                (c > 0x3020).then_some(0x3020)
            };
            if let Some(d) = base.and_then(|b| c.checked_sub(b)).filter(|d| *d <= 9) {
                return Some(d as u8);
            }
            if self.zero.len_utf16() != 1 {
                return None;
            }
        }
        c.checked_sub('0' as u32)
            .filter(|d| *d <= 9)
            .map(|d| d as u8)
    }
}

/// Letters of "inf" and "nan".
fn is_inf_nan_char(c: u8) -> bool {
    matches!(c, b'a' | b'f' | b'i' | b'n')
}

/// Qt's NumericTokenizer (DoubleScientificMode): one locale token of a number as ASCII
/// (`0` for none).
struct Tokens<'a> {
    text: &'a [char],
    i: usize,
    n: &'a Numeric,
}

impl Tokens<'_> {
    fn done(&self) -> bool {
        self.i >= self.text.len()
    }

    fn fraction_group_clash(&self) -> bool {
        !self.n.is_c && self.n.group == self.n.decimal
    }

    /// Length in chars of `pat` at the start of `tail`, if it is there.
    fn starts(tail: &[char], pat: &str, ignore_case: bool) -> Option<usize> {
        let mut k = 0;
        for p in pat.chars() {
            let c = *tail.get(k)?;
            let same = c == p || ignore_case && c.to_lowercase().eq(p.to_lowercase());
            if !same {
                return None;
            }
            k += 1;
        }
        (k > 0).then_some(k)
    }

    fn next(&mut self) -> u8 {
        let n = self.n;
        let ascii_lower = |c: char| {
            u8::try_from(c)
                .ok()
                .map(|b| if b >= b'A' { b | 0x20 } else { b })
        };
        loop {
            let tail = &self.text[self.i..];
            let ch = tail[0];
            if ch == '\u{2212}' {
                self.i += 1;
                return b'-';
            }
            if n.is_c {
                return match ascii_lower(ch) {
                    Some(a)
                        if a.is_ascii_digit()
                            || (b'+'..=b'.').contains(&a)
                            || is_inf_nan_char(a)
                            || a == b'e' =>
                    {
                        self.i += 1;
                        a
                    }
                    _ => 0,
                };
            }
            if let Some(a) = ascii_lower(ch)
                .filter(|a| a.is_ascii_digit() || *a == b'-' || *a == b'+' || is_inf_nan_char(*a))
            {
                self.i += 1;
                return a;
            }
            if let Some(k) = Self::starts(tail, n.minus, false) {
                self.i += k;
                return b'-';
            }
            if let Some(k) = Self::starts(tail, n.plus, false) {
                self.i += k;
                return b'+';
            }
            if let Some(k) = Self::starts(tail, n.group, false) {
                self.i += k;
                let decimal: Vec<char> = n.decimal.chars().collect();
                if self.fraction_group_clash()
                    && !tail[k..]
                        .windows(decimal.len().max(1))
                        .any(|w| w == decimal.as_slice())
                {
                    return b'.';
                }
                return b',';
            }
            if let Some(k) = Self::starts(tail, n.decimal, false) {
                self.i += k;
                return b'.';
            }
            if let Some(k) = Self::starts(tail, n.exponential, true) {
                self.i += k;
                return b'e';
            }
            if n.zero.len_utf16() == 1 {
                if ch.len_utf16() != 1 {
                    return 0;
                }
                if let Some(d) = n.digit_value(ch) {
                    self.i += 1;
                    return b'0' + d;
                }
            } else if ch.len_utf16() != 1 {
                return match n.digit_value(ch) {
                    Some(d) => {
                        self.i += 1;
                        b'0' + d
                    }
                    None => 0,
                };
            }
            match ch {
                // Arabic letter mark (before signs in standard Arabic), LRM, RLM.
                '\u{061C}' | '\u{200E}' | '\u{200F}' => {
                    self.i += 1;
                    if self.done() {
                        return 0;
                    }
                }
                ' ' if n.group == "\u{a0}" || n.group == "\u{202f}" => {
                    self.i += 1;
                    return b',';
                }
                // Cyrillic E is Ukrainian's exponent; others writing Cyrillic may use it.
                'E' | 'e' | '\u{0415}' | '\u{0435}' if n.cyrillic => {
                    self.i += 1;
                    return b'e';
                }
                _ => return 0,
            }
        }
    }
}

/// `qt_doubleToAscii` (double-conversion's DoubleToAscii) for finite `a >= 0`: digits
/// without trailing zeros and the decimal point position, `a ≈ 0.DIGITS × 10^decpt`,
/// rounded half up on the exact binary value.
fn double_to_ascii(a: f64, form: Form, precision: i32) -> (Vec<u8>, i32) {
    if a == 0.0 {
        return (vec![b'0'], 1);
    }
    let split = |s: String| {
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let digits: Vec<u8> = m.bytes().filter(u8::is_ascii_digit).collect();
        (digits, e.parse::<i32>().unwrap_or(0) + 1)
    };
    if precision == SHORTEST {
        return crate::value::shortest_digits(a);
    }
    // Exact: a double's decimal expansion has at most 767 significant digits.
    let (mut digits, mut decpt) = split(format!("{a:.800e}"));
    let trim = |d: &mut Vec<u8>| {
        while d.last() == Some(&b'0') {
            d.pop();
        }
    };
    trim(&mut digits);
    let keep = match form {
        Form::Decimal => i64::from(decpt) + i64::from(precision),
        Form::Exponent => i64::from(precision) + 1,
        Form::SignificantDigits => i64::from(precision.max(1)),
    };
    if keep < digits.len() as i64 {
        let up = keep >= 0 && digits[keep as usize] >= b'5';
        digits.truncate(keep.max(0) as usize);
        if up {
            loop {
                match digits.last_mut() {
                    Some(b'9') => {
                        digits.pop();
                    }
                    Some(d) => {
                        *d += 1;
                        break;
                    }
                    None => {
                        digits.push(b'1');
                        decpt += 1;
                        break;
                    }
                }
            }
        }
        trim(&mut digits);
        if digits.is_empty() {
            decpt = -precision;
        }
    }
    (digits, decpt)
}

/// QLocaleData::decimalForm over ASCII digits; `grouping` when digits are grouped.
fn decimal_form(
    mut digits: Vec<u8>,
    mut decpt: i32,
    precision: i32,
    pm: Pm,
    grouping: Option<(usize, usize, usize)>,
) -> Vec<u8> {
    if decpt < 0 {
        digits.splice(
            0..0,
            std::iter::repeat_n(b'0', decpt.unsigned_abs() as usize),
        );
        decpt = 0;
    } else if digits.len() < decpt as usize {
        digits.resize(decpt as usize, b'0');
    }
    let decpt = decpt as usize;
    let pad = match pm {
        Pm::DecimalDigits => decpt + precision.max(0) as usize,
        Pm::SignificantDigits => precision.max(0) as usize,
        Pm::ChopTrailingZeros => 0,
    };
    if digits.len() < pad {
        digits.resize(pad, b'0');
    }
    if decpt < digits.len() {
        digits.insert(decpt, b'.');
    }
    if let Some((first, higher, least)) = grouping
        && let Some(mut i) = decpt.checked_sub(least).filter(|i| *i >= first)
    {
        digits.insert(i, b',');
        while i > higher {
            i -= higher;
            digits.insert(i, b',');
        }
    }
    if decpt == 0 {
        digits.insert(0, b'0');
    }
    digits
}

/// QLocaleData::exponentForm's mantissa over ASCII digits.
fn exponent_form(mut digits: Vec<u8>, precision: i32, pm: Pm) -> Vec<u8> {
    let pad = match pm {
        Pm::DecimalDigits => precision.max(0) as usize + 1,
        Pm::SignificantDigits => precision.max(0) as usize,
        Pm::ChopTrailingZeros => 0,
    };
    if digits.len() < pad {
        digits.resize(pad, b'0');
    }
    if digits.len() > 1 {
        digits.insert(1, b'.');
    }
    digits
}

/// `qt_asciiToDouble` without trailing junk: `inf`/`nan` names, else double-conversion's
/// correctly rounded parse; overflow and underflow to zero are failures.
fn ascii_to_double(num: &[u8]) -> Option<f64> {
    if let [c, ..] = num
        && num.len() >= 3
        && b"-+IiNn".contains(c)
    {
        let has_sign = matches!(c, b'-' | b'+');
        let name = &num[usize::from(has_sign)..];
        if name[0] > b'9' {
            let name = name.to_ascii_lowercase();
            return match name.as_slice() {
                b"inf" if *c == b'-' => Some(f64::NEG_INFINITY),
                b"inf" => Some(f64::INFINITY),
                b"nan" if !has_sign => Some(f64::NAN),
                _ => None,
            };
        }
    }
    let d: f64 = std::str::from_utf8(num).ok()?.parse().ok()?;
    if !d.is_finite() {
        return None;
    }
    if d == 0.0 {
        for &c in num {
            match c {
                b'1'..=b'9' => return None,
                b'e' | b'E' => break,
                _ => {}
            }
        }
    }
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(name: &str) -> Numeric {
        Locale::named(name).numeric()
    }

    // Expected values from Qt 6.12 (PySide6) QML `Number` locale methods.
    #[test]
    fn to_string_matches_qt() {
        let us = n("en_US");
        assert_eq!(n("de_DE").to_string(1234.5, b'f', 2), "1.234,50");
        assert_eq!(n("de_DE").to_string(1234.5, b'f', 0), "1.235");
        assert_eq!(us.to_string(0.125, b'f', 2), "0.13");
        assert_eq!(us.to_string(1.005, b'f', 2), "1.00");
        assert_eq!(us.to_string(99.96, b'f', 1), "100.0");
        assert_eq!(us.to_string(-0.004, b'f', 2), "-0.00");
        assert_eq!(us.to_string(-0.0, b'f', 2), "0.00");
        assert_eq!(us.to_string(1.25, b'e', 1), "1.3e+00");
        assert_eq!(us.to_string(12.5, b'g', 2), "13");
        assert_eq!(us.to_string(123.456, b'g', 0), "1e+02");
        assert_eq!(us.to_string(1e-7, b'g', SHORTEST), "1e-07");
        assert_eq!(us.to_string(123456789.0, b'g', SHORTEST), "123,456,789");
        assert_eq!(us.to_string(0.0001, b'g', SHORTEST), "1e-04");
        assert_eq!(us.to_string(1e21, b'g', SHORTEST), "1e+21");
        assert_eq!(us.to_string(5e-324, b'g', 20), "4.9406564584124654418e-324");
        assert_eq!(us.to_string(0.1, b'f', 25), "0.1000000000000000055511151");
        assert_eq!(us.to_string(0.1, b'f', -1), "0.100000");
        assert_eq!(us.to_string(1e-10, b'G', 3), "1E-10");
        assert_eq!(us.to_string(f64::INFINITY, b'E', 2), "INF");
        assert_eq!(n("de_DE").to_string(f64::NEG_INFINITY, b'f', 2), "-inf");
        assert_eq!(n("C").to_string(1234567.891, b'f', 2), "1234567.89");
        assert_eq!(n("hi_IN").to_string(12345678.9, b'f', 2), "1,23,45,678.90");
        assert_eq!(n("ar_EG").to_string(1234.5, b'e', 2), "١٫٢٣أس؜+٠٣");
        assert_eq!(
            n("fa_IR").to_string(-1234.5, b'e', 2),
            "\u{200e}−۱٫۲۳×۱۰^\u{200e}+۰۳"
        );
    }

    #[test]
    fn currency_matches_qt() {
        assert_eq!(
            n("de_DE").to_currency_string(1234.5, None),
            "1.234,50\u{a0}€"
        );
        assert_eq!(
            n("en_US").to_currency_string(1234.5, Some("X")),
            "X1,234.50"
        );
        assert_eq!(n("en_US").to_currency_string(1.0, Some("")), "USD1.00");
        assert_eq!(n("en_US").to_currency_string(-1234.5, None), "($1,234.50)");
        assert_eq!(n("ja_JP").to_currency_string(1234.5, None), "￥1,235");
        assert_eq!(n("de_CH").to_currency_string(-1234.5, None), "CHF-1'234.50");
        assert_eq!(n("de_DE").to_currency_string(f64::NAN, None), "nan\u{a0}€");
        assert_eq!(n("C").to_currency_string(-1234.5, None), "-1234.50");
    }

    #[test]
    fn to_double_matches_qt() {
        let us = n("en_US");
        assert_eq!(us.to_double(" 1,234.5 "), Some(1234.5));
        assert_eq!(n("de_DE").to_double("1.234,5"), Some(1234.5));
        assert_eq!(n("fr_FR").to_double("1 234,5"), Some(1234.5));
        assert_eq!(n("ar_EG").to_double("١٬٢٣٤٫٥"), Some(1234.5));
        assert_eq!(n("ru_RU").to_double("1,5\u{435}3"), Some(1500.0));
        assert_eq!(n("fa_IR").to_double("\u{200e}\u{2212}۵"), Some(-5.0));
        assert_eq!(n("hi_IN").to_double("12,34,567.5"), Some(1234567.5));
        assert_eq!(n("hi_IN").to_double("1,234,567.5"), None);
        assert_eq!(us.to_double("1,2,3"), None);
        assert_eq!(us.to_double("1234,567"), None);
        assert_eq!(us.to_double("1,234e2"), Some(123400.0));
        assert_eq!(us.to_double("INF"), Some(f64::INFINITY));
        assert_eq!(us.to_double("-nan"), None);
        assert_eq!(us.to_double("Infinity"), None);
        assert_eq!(us.to_double("1e400"), None);
        assert_eq!(us.to_double("1e-400"), None);
        assert_eq!(us.to_double("1\u{200e}"), None);
        assert_eq!(us.to_double("١٢"), None);
        assert_eq!(us.to_double("5."), Some(5.0));
        assert_eq!(us.to_double(".5"), Some(0.5));
        assert_eq!(n("C").to_double("1,234.5"), Some(1234.5));
        assert_eq!(n("C").to_double("1,23"), None);
        assert!(
            us.to_double("-0")
                .is_some_and(|d| d == 0.0 && d.is_sign_negative())
        );
    }
}
