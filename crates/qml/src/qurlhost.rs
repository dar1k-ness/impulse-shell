//! The host of a url as Qt 6.12's QUrl holds it (qurl.cpp's `QUrlPrivate::setHost`,
//! qipaddress.cpp and qurlidna.cpp): an IPv4 or IPv6 address in its one spelling, a domain
//! name mapped and checked by UTS #46 and shown in Unicode only under the top-level domains
//! of Qt's list. The character data is ICU's, where Qt reads its own tables.
use icu_normalizer::uts46::Uts46Mapper;
use icu_properties::CodePointMapData;
use icu_properties::props::{
    BidiClass, CanonicalCombiningClass, GeneralCategory, GeneralCategoryGroup, JoiningType,
};

const BASE: u32 = 36;
const TMIN: u32 = 1;
const TMAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 128;
const MAX_LABEL: usize = 63;

/// `idn_whitelist`: the top-level domains whose names QUrl shows in Unicode.
const IDN_WHITELIST: &[&str] = &[
    "ac",
    "ar",
    "asia",
    "at",
    "biz",
    "br",
    "cat",
    "ch",
    "cl",
    "cn",
    "com",
    "de",
    "dk",
    "es",
    "fi",
    "gr",
    "hu",
    "il",
    "info",
    "io",
    "ir",
    "is",
    "jp",
    "kr",
    "li",
    "lt",
    "lu",
    "lv",
    "museum",
    "name",
    "net",
    "no",
    "nu",
    "nz",
    "org",
    "pl",
    "pr",
    "se",
    "sh",
    "tel",
    "th",
    "tm",
    "tw",
    "ua",
    "vn",
    "xn--fiqs8s",
    "xn--fiqz9s",
    "xn--fzc2c9e2c",
    "xn--j6w193g",
    "xn--kprw13d",
    "xn--kpry57d",
    "xn--mgba3a4f16a",
    "xn--mgba3a4fra",
    "xn--mgbaam7a8h",
    "xn--mgbayh7gpa",
    "xn--mgberp4a5d4ar",
    "xn--ogbpf8fl",
    "xn--p1ai",
    "xn--wgbh1c",
    "xn--wgbl6a",
    "xn--xkc2al3hye2a",
];

fn adapt(mut delta: u32, numpoints: u32, first: bool) -> u32 {
    delta /= if first { DAMP } else { 2 };
    delta += delta / numpoints;
    let mut k = 0;
    while delta > ((BASE - TMIN) * TMAX) / 2 {
        delta /= BASE - TMIN;
        k += BASE;
    }
    k + ((BASE - TMIN + 1) * delta) / (delta + SKEW)
}

fn threshold(k: u32, bias: u32) -> u32 {
    if k <= bias {
        TMIN
    } else if k >= bias + TMAX {
        TMAX
    } else {
        k - bias
    }
}

fn encode_digit(d: u32) -> char {
    // 0..25 are a..z, 26..35 are 0..9.
    (d + 22 + 75 * u32::from(d < 26)) as u8 as char
}

/// `qt_punycodeEncoder`: the label in ASCII, `xn--` ahead of an encoded one; `None` for a
/// label too long to encode.
fn punycode_encode(label: &str) -> Option<String> {
    if label.encode_utf16().count() > MAX_LABEL * 2 {
        return None;
    }
    if label.is_ascii() {
        return Some(label.to_string());
    }
    let mut out: String = label.chars().filter(char::is_ascii).collect();
    let basic = out.len() as u32;
    let mut h = basic;
    if h > 0 {
        out.push('-');
    }
    let len = label.chars().count() as u32;
    let (mut n, mut delta, mut bias) = (INITIAL_N, 0u32, INITIAL_BIAS);
    while h < len {
        let m = label.chars().map(u32::from).filter(|&c| c >= n).min()?;
        delta = delta.checked_add((m - n).checked_mul(h + 1)?)?;
        n = m;
        for c in label.chars().map(u32::from) {
            if c < n {
                delta = delta.checked_add(1)?;
            }
            if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = threshold(k, bias);
                    if q < t {
                        break;
                    }
                    out.push(encode_digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(encode_digit(q));
                bias = adapt(delta, h + 1, h == basic);
                delta = 0;
                h += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Some(format!("xn--{out}"))
}

/// `qt_punycodeDecoder`: what an `xn--` label spells, any other label as it is; `None` for
/// one that does not decode.
fn punycode_decode(label: &str) -> Option<String> {
    if label.len() > MAX_LABEL {
        return None;
    }
    if !label.starts_with("xn--") {
        return Some(label.to_string());
    }
    let b = label.as_bytes();
    let delimiter = label.rfind('-').unwrap_or(0);
    let mut out: Vec<char> = if delimiter < 4 {
        Vec::new()
    } else {
        label[4..delimiter].chars().collect()
    };
    let (mut n, mut i, mut bias) = (INITIAL_N, 0u32, INITIAL_BIAS);
    let mut at = delimiter + 1;
    while at < b.len() {
        let old = i;
        let mut w = 1u32;
        let mut k = BASE;
        while at < b.len() {
            let digit = match b[at] {
                c @ b'0'..=b'9' => u32::from(c - b'0') + 26,
                c @ b'A'..=b'Z' => u32::from(c - b'A'),
                c @ b'a'..=b'z' => u32::from(c - b'a'),
                _ => return None,
            };
            at += 1;
            i = i.checked_add(digit.checked_mul(w)?)?;
            let t = threshold(k, bias);
            if digit < t {
                break;
            }
            w = w.checked_mul(BASE - t)?;
            k += BASE;
        }
        let len = out.len() as u32 + 1;
        bias = adapt(i - old, len, old == 0);
        n = n.checked_add(i / len)?;
        i %= len;
        out.insert(i as usize, char::from_u32(n)?);
        i += 1;
    }
    Some(out.into_iter().collect())
}

fn ascii_label_char(c: char) -> bool {
    matches!(c, '-' | '_' | '0'..='9' | 'a'..='z')
}

/// `validateAsciiLabel`, for a label that is not empty.
fn valid_ascii_label(label: &str) -> bool {
    label.len() <= MAX_LABEL
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(ascii_label_char)
}

/// `idnaStatus(c)` is Valid, or Deviation (QUrl's processing is nontransitional).
fn idna_valid(c: char) -> bool {
    if c.is_ascii() {
        return ascii_label_char(c);
    }
    let mapper = Uts46Mapper::new();
    let mut mapped = mapper.normalize_validate(std::iter::once(c));
    c != '\u{fffd}' && mapped.next() == Some(c) && mapped.next().is_none()
}

/// `DomainValidityChecker::checkContextJRules` (RFC 5892 appendix A.1 and A.2).
fn context_j(label: &str) -> bool {
    #[derive(PartialEq)]
    enum State {
        Initial,
        LdT,
        ZwnjT,
    }
    let joining = CodePointMapData::<JoiningType>::new();
    let ccc = CodePointMapData::<CanonicalCombiningClass>::new();
    let mut state = State::Initial;
    let mut virama = false;
    for c in label.chars() {
        if c == '\u{200d}' {
            if !virama {
                return false;
            }
            state = State::Initial;
        } else if c == '\u{200c}' {
            if !virama && state != State::LdT {
                return false;
            }
            state = if virama { State::Initial } else { State::ZwnjT };
        } else {
            match joining.get(c) {
                JoiningType::LeftJoining => {
                    if state == State::ZwnjT {
                        return false;
                    }
                    state = State::LdT;
                }
                JoiningType::DualJoining => state = State::LdT,
                JoiningType::Transparent => {}
                _ => state = State::Initial,
            }
        }
        virama = ccc.get(c) == CanonicalCombiningClass::Virama;
    }
    state != State::ZwnjT
}

/// `DomainValidityChecker::checkBidiRules` (RFC 5893 section 2).
fn bidi_rules(label: &str) -> bool {
    let bidi = CodePointMapData::<BidiClass>::new();
    let mut chars = label.chars();
    let Some(first) = chars.next() else {
        return true;
    };
    let rtl = match bidi.get(first) {
        BidiClass::LeftToRight => false,
        BidiClass::RightToLeft | BidiClass::ArabicLetter => true,
        _ => return false,
    };
    let (mut tail_ok, mut en, mut an) = (true, false, false);
    for c in chars {
        match bidi.get(c) {
            BidiClass::RightToLeft | BidiClass::ArabicLetter => {
                if !rtl {
                    return false;
                }
                tail_ok = true;
            }
            BidiClass::LeftToRight => {
                if rtl {
                    return false;
                }
                tail_ok = true;
            }
            BidiClass::EuropeanSeparator
            | BidiClass::CommonSeparator
            | BidiClass::EuropeanTerminator
            | BidiClass::OtherNeutral
            | BidiClass::BoundaryNeutral => tail_ok = false,
            BidiClass::NonspacingMark => {}
            BidiClass::ArabicNumber => {
                if !rtl || en {
                    return false;
                }
                an = true;
                tail_ok = true;
            }
            BidiClass::EuropeanNumber => {
                if rtl {
                    if an {
                        return false;
                    }
                    en = true;
                }
                tail_ok = true;
            }
            _ => return false,
        }
    }
    tail_ok
}

/// `DomainValidityChecker`: the UTS #46 validity criteria over the labels of one name.
#[derive(Default)]
struct Checker {
    name_is_bidi: bool,
    had_bidi_errors: bool,
    ignore_bidi_errors: bool,
}

impl Checker {
    fn check_label(&mut self, label: &str) -> bool {
        if label.is_empty() {
            return true;
        }
        let nfc = icu_normalizer::ComposingNormalizerBorrowed::new_nfc();
        if !nfc.is_normalized(label) {
            return false;
        }
        // The third and fourth places are QString's, UTF-16 units.
        let units: Vec<u16> = label.encode_utf16().take(4).collect();
        if units.len() == 4 && units[2] == u16::from(b'-') && units[3] == u16::from(b'-') {
            return self.ignore_bidi_errors && label.starts_with("xn") && valid_ascii_label(label);
        }
        if label.starts_with('-') || label.ends_with('-') || label.contains('.') {
            return false;
        }
        let category = CodePointMapData::<GeneralCategory>::new();
        if label
            .chars()
            .next()
            .is_some_and(|c| GeneralCategoryGroup::Mark.contains(category.get(c)))
        {
            return false;
        }
        let bidi = CodePointMapData::<BidiClass>::new();
        let mut joiners = false;
        for c in label.chars() {
            joiners |= matches!(c, '\u{200c}' | '\u{200d}');
            if !self.ignore_bidi_errors
                && !self.name_is_bidi
                && matches!(
                    bidi.get(c),
                    BidiClass::RightToLeft | BidiClass::ArabicLetter | BidiClass::ArabicNumber
                )
            {
                self.name_is_bidi = true;
                if self.had_bidi_errors {
                    return false;
                }
            }
            if !idna_valid(c) {
                return false;
            }
        }
        if joiners && !context_j(label) {
            return false;
        }
        self.had_bidi_errors |= !bidi_rules(label);
        !(self.name_is_bidi && self.had_bidi_errors)
    }
}

/// `convertToUnicode`: the name with its `xn--` labels decoded, or the name itself when one
/// of them does not decode to a valid label.
fn to_unicode(ace: &str) -> String {
    let mut checker = Checker::default();
    let mut out = Vec::new();
    for label in ace.split('.') {
        match punycode_decode(label) {
            Some(u) if !u.is_empty() || label.is_empty() => {
                if !checker.check_label(&u) {
                    return ace.to_string();
                }
                out.push(u);
            }
            _ => return ace.to_string(),
        }
    }
    out.join(".")
}

/// `qt_ACE_do(domain, NormalizeAce, ForbidLeadingDot, {})`; `None` for a name that is not
/// valid.
fn normalize_ace(domain: &str) -> Option<String> {
    let name_char = |c: char| ascii_label_char(c) || c == '.';
    // mapDomainName: a name of these characters, in either case, is only lowercased.
    let ace = if domain.chars().all(|c| name_char(c.to_ascii_lowercase())) {
        domain.to_ascii_lowercase()
    } else {
        // The mapping and NFC; what is disallowed comes out as U+FFFD.
        let mapped: String = Uts46Mapper::new().map_normalize(domain.chars()).collect();
        let mut checker = Checker {
            ignore_bidi_errors: true,
            ..Checker::default()
        };
        if mapped.is_empty() || !mapped.split('.').all(|l| checker.check_label(l)) {
            return None;
        }
        // convertToAscii.
        let labels: Vec<&str> = mapped.split('.').collect();
        let mut out = Vec::with_capacity(labels.len());
        for (i, label) in labels.iter().enumerate() {
            if label.is_empty() && i + 1 != labels.len() {
                return None;
            }
            out.push(punycode_encode(label)?);
        }
        out.join(".")
    };
    if ace.is_empty() {
        return None;
    }
    // checkAsciiDomainName: only the last label may be empty.
    let labels: Vec<&str> = ace.split('.').collect();
    for (i, label) in labels.iter().enumerate() {
        if label.is_empty() {
            if i + 1 != labels.len() {
                return None;
            }
        } else if !valid_ascii_label(label) {
            return None;
        }
    }
    let punycode = labels.iter().any(|l| l.starts_with("xn--"));
    // qt_is_idn_enabled: its `equal` holds for the empty name after a last dot.
    let tld = ace.rfind('.').map(|i| &ace[i + 1..]);
    if punycode && tld.is_some_and(|t| t.is_empty() || IDN_WHITELIST.contains(&t)) {
        Some(to_unicode(&ace))
    } else {
        Some(ace)
    }
}

/// `qt_ACE_do(host, ToAceOnly, AllowLeadingDot, {})` of a host held as QUrl holds it: its
/// labels above ASCII in punycode; nothing for a label too long to encode.
pub(crate) fn ace(host: &str) -> String {
    if host.is_ascii() {
        return host.to_string();
    }
    let labels: Option<Vec<String>> = host.split('.').map(punycode_encode).collect();
    labels.map(|l| l.join(".")).unwrap_or_default()
}

/// `qstrntoull(s, base)`: the number `s` starts with and how many bytes spell it.
fn strtoull(s: &[u8], mut base: u32) -> Option<(u64, usize)> {
    let mut p = s
        .iter()
        .position(|c| !matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))?;
    if s[p] == b'-' {
        return None;
    }
    if s[p] == b'+' {
        p += 1;
    }
    // scanPrefix.
    if s.get(p).is_some_and(u8::is_ascii_digit) {
        if s[p] == b'0' {
            match s.get(p + 1) {
                Some(b'b' | b'B') => {
                    if base == 0 {
                        base = 2;
                    }
                    if base == 2 {
                        p += 2;
                    }
                }
                Some(b'x' | b'X') => {
                    if base == 0 {
                        base = 16;
                    }
                    if base == 16 {
                        p += 2;
                    }
                }
                _ if base == 0 => base = 8,
                _ => {}
            }
        } else if base == 0 {
            base = 10;
        }
    }
    if base == 0 || p >= s.len() {
        return None;
    }
    let mut value = 0u64;
    let mut end = p;
    while let Some(d) = s.get(end).and_then(|&c| (c as char).to_digit(base)) {
        value = value
            .checked_mul(u64::from(base))?
            .checked_add(u64::from(d))?;
        end += 1;
    }
    Some((value, if end == p { 0 } else { end }))
}

/// `QIPAddressUtils::parseIp4Internal`: one to four numbers, the last filling what is left.
fn parse_ip4(s: &str, leading_zero: bool) -> Option<u32> {
    if s.bytes().any(|c| c >= 0x7f) {
        return None;
    }
    let mut b = s.as_bytes();
    let mut address = 0u32;
    for dots in 0..4 {
        if !leading_zero && b.first() == Some(&b'0') && !matches!(b.get(1), None | Some(b'.')) {
            return None;
        }
        let (ll, used) = strtoull(b, 0)?;
        let x = u32::try_from(ll).ok()?;
        if used == 0 {
            return None;
        }
        let next = b.get(used).copied();
        let (limit, shift) = if next == Some(b'.') || dots == 3 {
            (0xff, 8)
        } else {
            match dots {
                2 => (0xffff, 16),
                1 => (0xff_ffff, 24),
                _ => (u32::MAX, 0),
            }
        };
        if x & !limit != 0 {
            return None;
        }
        address = address << shift | x;
        if dots == 3 || next.is_none() {
            return next.is_none().then_some(address);
        }
        if next != Some(b'.') {
            return None;
        }
        b = &b[used + 1..];
    }
    None
}

fn ip4_string(a: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        a >> 24,
        (a >> 16) & 255,
        (a >> 8) & 255,
        a & 255
    )
}

/// `QIPAddressUtils::parseIp6`.
fn parse_ip6(s: &str) -> Option<[u8; 16]> {
    if s.len() < 2 || s.bytes().any(|c| c >= 0x7f) {
        return None;
    }
    let b = s.as_bytes();
    let colons = b.iter().filter(|&&c| c == b':').count();
    let dots = b.iter().filter(|&&c| c == b'.').count();
    if dots != 0 && dots != 3 {
        return None;
    }
    let mut address = [0u8; 16];
    if colons == 2 && b.len() == 2 {
        return Some(address);
    }
    let mut fill = if b.starts_with(b"::") || b.ends_with(b"::") {
        9 - colons as isize
    } else if !(2..=7).contains(&colons) {
        return None;
    } else {
        8 - colons as isize
    };
    if dots != 0 {
        fill -= 1;
    }
    let at = |i: usize| b.get(i).copied();
    let mut p = 0;
    let mut pos = 0;
    while pos < 15 {
        if at(p) == Some(b':') {
            if fill < 1 {
                return None;
            }
            if pos == 0 || pos == colons * 2 {
                if at(p + 1) != Some(b':') {
                    return None;
                }
                p += 1;
            }
            pos += fill as usize * 2;
            fill = 0;
            p += 1;
            continue;
        }
        let (ll, used) = strtoull(&b[p.min(b.len())..], 16)?;
        if used == 0 || used > 4 {
            return None;
        }
        if at(p + used) == Some(b'.') {
            if pos != 12 {
                return None;
            }
            let ip4 = parse_ip4(&s[p..], false)?;
            address[12..].copy_from_slice(&ip4.to_be_bytes());
            return Some(address);
        }
        let x = ll as u16;
        *address.get_mut(pos)? = (x >> 8) as u8;
        *address.get_mut(pos + 1)? = x as u8;
        pos += 2;
        match at(p + used) {
            None => break,
            Some(b':') => p += used + 1,
            Some(_) => return None,
        }
    }
    (pos == 16).then_some(address)
}

/// `QIPAddressUtils::toString(IPv6Address)`.
fn ip6_string(a: &[u8; 16]) -> String {
    let word = |i: usize| u16::from(a[i]) << 8 | u16::from(a[i + 1]);
    let mut embedded = false;
    if a[..10].iter().all(|&c| c == 0) {
        if a[10] == 0xff && a[11] == 0xff {
            embedded = true;
        } else if a[10] == 0 && a[11] == 0 {
            if a[12] != 0 || a[13] != 0 || a[14] != 0 {
                embedded = true;
            } else if a[15] == 0 {
                return "::".to_string();
            }
        }
    }
    let (mut run, mut run_at) = (0, 0);
    let mut i = 0;
    while i < 16 {
        if word(i) == 0 {
            let mut j = i;
            while j < 16 && word(j) == 0 {
                j += 2;
            }
            if j - i > run {
                run = j - i;
                run_at = i;
                i = j;
            }
        }
        i += 2;
    }
    let mut out = String::new();
    let run_at = if run < 4 {
        None
    } else {
        if run_at == 0 {
            out.push(':');
        }
        Some(run_at)
    };
    let mut i = 0;
    while i < 16 {
        if Some(i) == run_at {
            out.push(':');
            i += run;
            continue;
        }
        if i == 12 && embedded {
            out.push_str(&ip4_string(u32::from(word(12)) << 16 | u32::from(word(14))));
            return out;
        }
        out.push_str(&format!("{:x}", word(i)));
        if i != 14 {
            out.push(':');
        }
        i += 2;
    }
    out
}

/// qurl.cpp `parseIpFuture`, for `[vX.…]` longer than five characters; an address that does
/// not parse leaves what was read of it as the host.
fn ip_future(s: &str, tolerant: bool) -> Result<String, String> {
    let b = s.as_bytes();
    if b[3] != b'.' || !b[2].is_ascii_hexdigit() {
        return Err(String::new());
    }
    let mut out = format!("[v{}.", (b[2] as char).to_ascii_uppercase());
    let rest = &b[4..b.len() - 1];
    let decoded = tolerant.then(|| fully_decoded(rest)).flatten();
    for &c in decoded.as_deref().unwrap_or(rest) {
        if !(c.is_ascii_alphanumeric() || b"!$&'()*+,;=:-._~".contains(&c)) {
            return Err(out);
        }
        out.push(c as char);
    }
    out.push(']');
    Ok(out)
}

/// qurlrecode.cpp `decode` (`qt_urlRecode` to FullyDecoded): every escape as its byte, or
/// nothing decoded when one of them is not two hex digits.
fn fully_decoded(b: &[u8]) -> Option<Vec<u8>> {
    let hex = |c: Option<&u8>| c.and_then(|&c| (c as char).to_digit(16));
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push((hex(b.get(i + 1))? * 16 + hex(b.get(i + 2))?) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

/// qurl.cpp `parseIp6`: the address between the brackets, a zone after `%25` kept.
fn ip6(s: &str, tolerant: bool, recode: &dyn Fn(&str, &'static str) -> String) -> Option<String> {
    let decoded = if tolerant {
        recode(s, ":")
    } else {
        s.to_string()
    };
    let (address, zone) = match decoded.find("%25") {
        Some(i) if decoded.rfind("%25") == Some(i) => {
            let zone = &decoded[i + 3..];
            if zone.is_empty() {
                return None;
            }
            (&decoded[..i], Some(zone))
        }
        _ => (&decoded[..], None),
    };
    let mut out = format!("[{}", ip6_string(&parse_ip6(address)?));
    if let Some(zone) = zone {
        out.push_str("%25");
        out.push_str(zone);
    }
    out.push(']');
    Some(out)
}

/// `QUrlPrivate::setHost`: the host QUrl holds for the one written; `Err` for a host that
/// makes the url invalid, with what QUrl holds then. `recode(s, decode)` is `qt_urlRecode`
/// with `decode` decoded too.
pub(crate) fn host(
    raw: &str,
    tolerant: bool,
    recode: &dyn Fn(&str, &'static str) -> String,
) -> Result<String, String> {
    if raw.is_empty() {
        return Ok(String::new());
    }
    if raw.starts_with('[') {
        if !raw.ends_with(']') {
            return Err(String::new());
        }
        let b = raw.as_bytes();
        if b.get(1) == Some(&b'v') {
            if b.len() > 5 {
                return ip_future(raw, tolerant);
            }
            return Err(String::new());
        }
        return ip6(&raw[1..raw.len() - 1], tolerant, recode).ok_or_else(String::new);
    }
    if let Some(a) = parse_ip4(raw, true) {
        return Ok(ip4_string(a));
    }
    if tolerant {
        // An escaped host may spell any of the kinds above.
        let s = recode(raw, "");
        if s != raw {
            return if s.contains('%') {
                Err(String::new())
            } else {
                host(&s, false, recode)
            };
        }
    }
    let s = normalize_ace(raw).ok_or_else(String::new)?;
    Ok(parse_ip4(&s, true).map_or(s, ip4_string))
}

/// The character data against Qt 6.12's: `tests/qt_unicode/qt.tsv` holds what Qt and the ICU
/// beside it give for the code points of `points.txt` (`dump.sh` writes it).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::qurl::QUrl;
    use icu_normalizer::{ComposingNormalizerBorrowed, DecomposingNormalizerBorrowed};

    const QT: &str = include_str!("../tests/qt_unicode/qt.tsv");

    fn rows(kind: &str) -> Vec<Vec<&'static str>> {
        let rows: Vec<Vec<&str>> = (QT.lines().map(|l| l.split('\t').collect()))
            .filter(|f: &Vec<&str>| f[0] == kind)
            .collect();
        assert!(!rows.is_empty(), "no {kind} rows");
        rows
    }

    fn point(hex: &str) -> char {
        char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap()
    }

    fn text(hex: &str) -> String {
        hex.split(' ').map(point).collect()
    }

    fn hex(s: &str) -> String {
        let points: Vec<String> = s.chars().map(|c| format!("{:X}", c as u32)).collect();
        points.join(" ")
    }

    fn assigned(c: char) -> bool {
        CodePointMapData::<GeneralCategory>::new().get(c) != GeneralCategory::Unassigned
    }

    /// Qt's name of a Bidi_Class (`QChar::Direction`).
    fn bidi_class(c: char) -> &'static str {
        const NAMES: &[(BidiClass, &str)] = &[
            (BidiClass::LeftToRight, "L"),
            (BidiClass::RightToLeft, "R"),
            (BidiClass::EuropeanNumber, "EN"),
            (BidiClass::EuropeanSeparator, "ES"),
            (BidiClass::EuropeanTerminator, "ET"),
            (BidiClass::ArabicNumber, "AN"),
            (BidiClass::CommonSeparator, "CS"),
            (BidiClass::ParagraphSeparator, "B"),
            (BidiClass::SegmentSeparator, "S"),
            (BidiClass::WhiteSpace, "WS"),
            (BidiClass::OtherNeutral, "ON"),
            (BidiClass::LeftToRightEmbedding, "LRE"),
            (BidiClass::LeftToRightOverride, "LRO"),
            (BidiClass::ArabicLetter, "AL"),
            (BidiClass::RightToLeftEmbedding, "RLE"),
            (BidiClass::RightToLeftOverride, "RLO"),
            (BidiClass::PopDirectionalFormat, "PDF"),
            (BidiClass::NonspacingMark, "NSM"),
            (BidiClass::BoundaryNeutral, "BN"),
            (BidiClass::LeftToRightIsolate, "LRI"),
            (BidiClass::RightToLeftIsolate, "RLI"),
            (BidiClass::FirstStrongIsolate, "FSI"),
            (BidiClass::PopDirectionalIsolate, "PDI"),
        ];
        let class = CodePointMapData::<BidiClass>::new().get(c);
        NAMES.iter().find(|(k, _)| *k == class).unwrap().1
    }

    /// The urls of the three hosts `probe.cpp` makes of a code point.
    fn urls(c: char) -> [String; 3] {
        [
            format!("{c}.com"),
            format!("a{c}b.com"),
            format!("\u{5d0}{c}\u{5d0}.com"),
        ]
        .map(|host| QUrl::parse(&format!("http://{host}/")).to_string())
    }

    /// ICU4X does not tell the Unicode version of its data. The number of assigned code points
    /// does: every version assigns more.
    #[test]
    fn the_data_is_of_qt_s_unicode_version() {
        let unicode = &rows("unicode")[0];
        let (version, qt, icu) = (unicode[1], unicode[2].parse::<usize>().unwrap(), unicode[3]);
        assert_eq!(version, icu, "Qt's tables and the ICU beside it");
        let ours = ('\0'..=char::MAX).filter(|c| assigned(*c)).count();
        assert_eq!(
            ours, qt,
            "code points assigned in the data of icu_properties, in Unicode {version} of Qt 6.12"
        );
    }

    /// What Unicode 18 adds or changes is as in Qt's Unicode 17: a code point it adds is not
    /// assigned and no host holds it, and the three whose Bidi_Class it changes keep the old one.
    #[test]
    fn code_points_of_the_next_unicode_are_qt_s() {
        let mut wrong = Vec::new();
        let (added, changed) = (rows("added"), rows("bidi"));
        assert!(added.len() > 80 && changed.len() == 3);
        for row in added.iter().chain(&changed) {
            let c = point(row[1]);
            let known = row[3] == "1";
            // The class of a code point that is not assigned is not compared: Qt's tables give R
            // to all of U+10800..U+10FFF where DerivedBidiClass.txt gives AL to U+10EC0..U+10EFF,
            // and no host holds such a code point.
            let class = if known { bidi_class(c) } else { row[4] };
            let ours = (assigned(c), class, urls(c));
            let qt = (known, row[4], [row[5], row[6], row[7]].map(String::from));
            if ours != qt {
                wrong.push(format!("U+{} ({}): {ours:?}, Qt {qt:?}", row[1], row[2]));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} differ\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    /// docs/qt-differences.md, "Six code points QString::normalized does not compose": the
    /// normalization is ICU's, in a host too.
    #[test]
    fn six_code_points_compose_as_in_icu_where_qt_does_not() {
        let (nfc, nfd) = (
            ComposingNormalizerBorrowed::new_nfc(),
            DecomposingNormalizerBorrowed::new_nfd(),
        );
        let (nfkc, nfkd) = (
            ComposingNormalizerBorrowed::new_nfkc(),
            DecomposingNormalizerBorrowed::new_nfkd(),
        );
        let mut differ = std::collections::BTreeMap::new();
        for row in rows("normalized") {
            let (input, form, qt, icu) = (text(row[2]), row[3], row[4], row[5]);
            let ours = match form {
                "NFC" => nfc.normalize(&input),
                "NFD" => nfd.normalize(&input),
                "NFKC" => nfkc.normalize(&input),
                "NFKD" => nfkd.normalize(&input),
                _ => unreachable!(),
            };
            assert_eq!(hex(&ours), icu, "{form} of {}", row[2]);
            if qt != icu {
                assert!(form == "NFC" || form == "NFKC", "{form} of {}", row[2]);
                *differ.entry(row[1]).or_insert(0) += 1;
            }
        }
        let points: Vec<&str> = differ.keys().copied().collect();
        assert_eq!(
            points,
            ["105C9", "105E4", "16126", "16127", "16128", "16D6A"]
        );
        assert_eq!(differ.values().sum::<i32>(), 30, "{differ:?}");

        let mut hosts = 0;
        for row in rows("nfc") {
            let c = point(row[1]);
            assert_eq!(
                (assigned(c), bidi_class(c)),
                (row[3] == "1", row[4]),
                "U+{}",
                row[1]
            );
            for (ours, qt) in urls(c).iter().zip(&row[5..8]) {
                assert_eq!(*ours, nfc.normalize(qt), "U+{}", row[1]);
                hosts += usize::from(ours != qt);
            }
        }
        // U+16126..U+16128 in the two hosts Qt takes.
        assert_eq!(hosts, 6);
    }
}
