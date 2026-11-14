//! A KSyntaxHighlighting `RegExpr` pattern, written for PCRE2 as `QRegularExpression` compiles
//! it with `UseUnicodePropertiesOption`, in the dialect of `fancy-regex`.

/// What a POSIX class matches under PCRE2's `UCP`, as the inside of a character class.
fn posix(name: &str, negated: bool) -> Option<String> {
    let set = match name {
        "alpha" => r"\p{L}",
        "alnum" => r"\p{L}\p{N}",
        "ascii" => r"\x00-\x7f",
        "digit" => r"\p{Nd}",
        "lower" => r"\p{Ll}",
        "upper" => r"\p{Lu}",
        "space" => r"\s",
        "blank" => r"\t\p{Zs}",
        "word" => r"\p{L}\p{N}_",
        "cntrl" => r"\p{Cc}",
        "punct" => r"\p{P}\$+<=>^`|~",
        "xdigit" => "0-9A-Fa-f",
        "graph" => r"\p{L}\p{M}\p{N}\p{P}\p{S}\p{Cf}",
        "print" => r"\p{L}\p{M}\p{N}\p{P}\p{S}\p{Zs}\p{Cf}",
        _ => return None,
    };
    Some(if negated {
        format!("[^{set}]")
    } else {
        set.to_string()
    })
}

const VERTICAL: &str = r"\n\x0b\x0c\r\x{85}\x{2028}\x{2029}";
const HORIZONTAL: &str = r"\t\p{Zs}";

/// `QRegularExpression::escape`: every character but `[A-Za-z0-9_]` behind a backslash.
pub fn escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The same pattern for `fancy-regex`; `None` when it uses what the dialect lacks.
fn translate(pattern: &str, insensitive: bool, minimal: bool) -> Option<String> {
    let p: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    if insensitive {
        out += "(?i)";
    }
    if minimal {
        out += "(?U)";
    }
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = 0;
    while i < p.len() {
        let c = p[i];
        if c == '\\' {
            let n = *p.get(i + 1)?;
            i += 2;
            match n {
                'Q' => {
                    let end = (i..p.len())
                        .find(|&j| p[j] == '\\' && p.get(j + 1) == Some(&'E'))
                        .unwrap_or(p.len());
                    out += &escape(&p[i..end].iter().collect::<String>());
                    i = (end + 2).min(p.len());
                }
                'E' => {}
                'h' if in_class => out += HORIZONTAL,
                'h' => out += &format!("[{HORIZONTAL}]"),
                'H' if !in_class => out += &format!("[^{HORIZONTAL}]"),
                'v' if in_class => out += VERTICAL,
                'v' => out += &format!("[{VERTICAL}]"),
                'V' if !in_class => out += &format!("[^{VERTICAL}]"),
                'R' if !in_class => out += &format!(r"(?:\r\n|[{VERTICAL}])"),
                'N' if !in_class => out += r"[^\n]",
                'H' | 'V' | 'R' | 'N' | 'X' | 'C' => return None,
                'e' => out += r"\x1b",
                'a' => out += r"\x07",
                'b' if in_class => out += r"\x08",
                'c' => {
                    let x = p.get(i)?.to_ascii_uppercase() as u32 ^ 0x40;
                    out += &format!(r"\x{{{x:x}}}");
                    i += 1;
                }
                '0' => {
                    let mut v = 0;
                    let mut k = 0;
                    while k < 2 && p.get(i).is_some_and(|d| ('0'..='7').contains(d)) {
                        v = v * 8 + p[i].to_digit(8).unwrap();
                        i += 1;
                        k += 1;
                    }
                    out += &format!(r"\x{{{v:x}}}");
                }
                _ if n.is_ascii_alphanumeric() => {
                    out.push('\\');
                    out.push(n);
                }
                // An escaped character that is not a letter or a digit stands for itself;
                // `\<` and `\>` are word boundaries in fancy-regex.
                '<' | '>' | ' ' | '"' | '\'' | '%' | ':' | ';' | '=' | '@' | '_' | '`' | ','
                | '!' => out.push(n),
                _ if !n.is_ascii() => out.push(n),
                _ => {
                    out.push('\\');
                    out.push(n);
                }
            }
            continue;
        }
        if in_class {
            match c {
                ']' if i > class_start => {
                    in_class = false;
                    out.push(']');
                }
                ']' => out += r"\]",
                '[' if p.get(i + 1) == Some(&':') => {
                    let end = (i + 2..p.len().saturating_sub(1))
                        .find(|&j| p[j] == ':' && p[j + 1] == ']');
                    match end {
                        Some(end) => {
                            let name: String = p[i + 2..end].iter().collect();
                            let (name, neg) = match name.strip_prefix('^') {
                                Some(n) => (n.to_string(), true),
                                None => (name, false),
                            };
                            out += &posix(&name, neg)?;
                            i = end + 2;
                            continue;
                        }
                        None => out += r"\[",
                    }
                }
                '[' => out += r"\[",
                '&' | '~' | '-' if p.get(i + 1) == Some(&c) && p.get(i + 2) != Some(&']') => {
                    out.push(c);
                    out.push('\\');
                    out.push(c);
                    i += 1;
                }
                _ => out.push(c),
            }
            i += 1;
            continue;
        }
        match c {
            '[' => {
                in_class = true;
                out.push('[');
                i += 1;
                if p.get(i) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                class_start = i;
                continue;
            }
            '{' => {
                // A brace that does not open `{n}`, `{n,}`, `{n,m}` or `{,m}` is a brace.
                let end = (i + 1..p.len()).find(|&j| p[j] == '}');
                let body: String = end
                    .map(|e| p[i + 1..e].iter().collect())
                    .unwrap_or_default();
                let (a, b) = body.split_once(',').unwrap_or((&body, "x"));
                let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
                let quantifier = end.is_some()
                    && digits(a)
                    && (b == "x" || digits(b))
                    && !(a.is_empty() && (b.is_empty() || b == "x"));
                if !quantifier {
                    out += r"\{";
                } else if a.is_empty() {
                    out += &format!("{{0,{b}}}");
                    i = end.unwrap();
                } else {
                    out.push('{');
                }
            }
            '}' if !out.ends_with(|c: char| c.is_ascii_digit() || c == ',') => out += r"\}",
            _ => out.push(c),
        }
        i += 1;
    }
    Some(out)
}

/// How a group opens.
enum Open {
    /// A capturing group with the number PCRE2 gives it.
    Capture(usize),
    /// `(?|`: every alternative numbers its groups from the same number.
    Reset,
    Plain,
}

struct Group {
    open: usize,
    /// The length of what opens it: 1 for `(`, 3 for `(?:`.
    head: usize,
    close: usize,
    kind: Open,
}

/// `(?R)` and `(?1)`: the pattern (0) or a group run again where it stands.
struct Call {
    at: usize,
    len: usize,
    target: usize,
}

/// The groups and the calls of a pattern.
fn structure(p: &[char]) -> Option<(Vec<Group>, Vec<Call>)> {
    let mut groups: Vec<Group> = vec![];
    let mut calls = vec![];
    // (group, the number its alternatives start from, the highest number an alternative reached)
    let mut open: Vec<(usize, usize, usize)> = vec![];
    let mut next = 1;
    let mut i = 0;
    while i < p.len() {
        match p[i] {
            '\\' if p.get(i + 1) == Some(&'Q') => {
                i = (i + 2..p.len())
                    .find(|&j| p[j] == '\\' && p.get(j + 1) == Some(&'E'))
                    .map_or(p.len(), |j| j + 2);
                continue;
            }
            '\\' => i += 1,
            '[' => {
                i += 1;
                if p.get(i) == Some(&'^') {
                    i += 1;
                }
                if p.get(i) == Some(&']') {
                    i += 1;
                }
                while i < p.len() && p[i] != ']' {
                    if p[i] == '\\' {
                        i += 1;
                    } else if p[i] == '[' && p.get(i + 1) == Some(&':') {
                        let end = (i + 2..p.len().saturating_sub(1))
                            .find(|&j| p[j] == ':' && p[j + 1] == ']');
                        i = end.map_or(i, |e| e + 1);
                    }
                    i += 1;
                }
            }
            '(' => {
                let rest: String = p[i + 1..p.len().min(i + 12)].iter().collect();
                let digits = rest
                    .strip_prefix('?')
                    .map_or(0, |r| r.chars().take_while(char::is_ascii_digit).count());
                if rest.starts_with("?R)") || (digits > 0 && rest[1 + digits..].starts_with(')')) {
                    let target = rest[1..1 + digits].parse().unwrap_or(0);
                    let len = 3 + digits.max(1);
                    calls.push(Call { at: i, len, target });
                    i += len;
                    continue;
                }
                let named = ["?<", "?P<", "?'"].iter().any(|n| rest.starts_with(n))
                    && !rest.starts_with("?<=")
                    && !rest.starts_with("?<!");
                let (kind, head) = if rest.starts_with("?|") {
                    (Open::Reset, 3)
                } else if named {
                    let end = rest.find(['>', '\''].as_slice()).filter(|&e| e > 2)?;
                    (Open::Capture(next), end + 2)
                } else if rest.starts_with('?') || rest.starts_with('*') {
                    (Open::Plain, 1)
                } else {
                    (Open::Capture(next), 1)
                };
                if matches!(kind, Open::Capture(_)) {
                    next += 1;
                }
                open.push((groups.len(), next, next));
                groups.push(Group {
                    open: i,
                    head,
                    close: p.len(),
                    kind,
                });
            }
            ')' => {
                let (g, _, highest) = open.pop()?;
                groups[g].close = i;
                if matches!(groups[g].kind, Open::Reset) {
                    next = next.max(highest);
                }
            }
            '|' => {
                if let Some((g, start, highest)) = open.last_mut()
                    && matches!(groups[*g].kind, Open::Reset)
                {
                    *highest = (*highest).max(next);
                    next = *start;
                }
            }
            _ => {}
        }
        i += 1;
    }
    open.is_empty().then_some((groups, calls))
}

/// The longest pattern the calls of a pattern are written out to.
const LONGEST: usize = 24_000;

/// `p[from..to]` with `(?|` as a plain group and every call replaced by what it calls,
/// `depth` calls deep; a deeper call matches nothing. `copy` is the text of a call, whose
/// groups capture nothing. `false` when the text passes `LONGEST`.
fn written(
    p: &[char],
    parts: &(Vec<Group>, Vec<Call>),
    (from, to): (usize, usize),
    depth: usize,
    copy: bool,
    out: &mut String,
) -> bool {
    let (groups, calls) = parts;
    let mut i = from;
    while i < to {
        if out.len() > LONGEST {
            return false;
        }
        if let Some(call) = calls.iter().find(|c| c.at == i) {
            i += call.len;
            let target = match call.target {
                0 => Some((0, p.len())),
                n => groups
                    .iter()
                    .find(|g| matches!(g.kind, Open::Capture(k) if k == n))
                    .map(|g| (g.open + g.head, g.close)),
            };
            match target {
                Some(range) if depth > 0 => {
                    *out += "(?:";
                    if !written(p, parts, range, depth - 1, true, out) {
                        return false;
                    }
                    out.push(')');
                }
                // Nothing matches it, and unlike `(?!)` it can be repeated.
                _ => *out += r"[^\s\S]",
            }
            continue;
        }
        match groups.iter().find(|g| g.open == i) {
            Some(g)
                if matches!(g.kind, Open::Reset)
                    || (copy && matches!(g.kind, Open::Capture(_))) =>
            {
                *out += "(?:";
                i += g.head;
            }
            _ => {
                out.push(p[i]);
                i += 1;
            }
        }
    }
    true
}

/// How deep the calls of a pattern are written out at most.
const DEEPEST: usize = 12;

/// A pattern of a syntax description, compiled.
pub struct Regex {
    re: fancy_regex::Regex,
    /// For each group PCRE2 numbers, the groups of `re` that stand for it; empty when the
    /// numbers are the same.
    groups: Vec<Vec<usize>>,
}

impl Regex {
    pub fn new(pattern: &str, insensitive: bool, minimal: bool) -> Option<Regex> {
        let p: Vec<char> = pattern.chars().collect();
        let parts = structure(&p)?;
        let special = !parts.1.is_empty() || parts.0.iter().any(|g| matches!(g.kind, Open::Reset));
        if !special {
            let re = fancy_regex::Regex::new(&translate(pattern, insensitive, minimal)?).ok()?;
            return Some(Regex { re, groups: vec![] });
        }
        let mut groups: Vec<Vec<usize>> = vec![];
        let mut number = 0;
        for g in &parts.0 {
            if let Open::Capture(n) = g.kind {
                number += 1;
                if groups.len() < n {
                    groups.resize(n, vec![]);
                }
                groups[n - 1].push(number);
            }
        }
        (1..=DEEPEST).rev().find_map(|depth| {
            let mut text = String::new();
            if !written(&p, &parts, (0, p.len()), depth, false, &mut text) {
                return None;
            }
            let re = fancy_regex::Regex::new(&translate(&text, insensitive, minimal)?).ok()?;
            Some(Regex {
                re,
                groups: groups.clone(),
            })
        })
    }

    /// The first match at or after byte `pos`: where it starts and ends, and the texts of
    /// the groups up to the last one that took part.
    pub fn find(&self, text: &str, pos: usize) -> Option<(usize, usize, Vec<String>)> {
        let c = self.re.captures_from_pos(text, pos).ok().flatten()?;
        let m = c.get(0)?;
        let group = |n: usize| -> Option<&str> {
            if self.groups.is_empty() {
                return c.get(n).map(|g| g.as_str());
            }
            self.groups[n - 1]
                .iter()
                .find_map(|&k| c.get(k))
                .map(|g| g.as_str())
        };
        let count = if self.groups.is_empty() {
            c.len() - 1
        } else {
            self.groups.len()
        };
        let last = (1..=count).rev().find(|&n| group(n).is_some()).unwrap_or(0);
        let texts = (1..=last)
            .map(|n| group(n).unwrap_or("").to_string())
            .collect();
        Some((m.start(), m.end(), texts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_rewritten_for_fancy_regex() {
        for (from, to) in [
            (r"[^[\]]+", r"[^\[\]]+"),
            (r"[[:alpha:]_][[:alnum:]_]*", r"[\p{L}_][\p{L}\p{N}_]*"),
            (r"\h+a{b", r"[\t\p{Zs}]+a\{b"),
            (r"a{2,3}\<", r"a{2,3}<"),
            (r"[]a]", r"[\]a]"),
            (r"\Qa.b\E.", r"a\.b."),
        ] {
            assert_eq!(translate(from, false, false).as_deref(), Some(to), "{from}");
        }
        assert_eq!(translate("a+", true, true).as_deref(), Some("(?i)(?U)a+"));
        assert_eq!(escape("a.b"), r"a\.b");
    }

    fn found(pattern: &str, text: &str) -> Option<(usize, usize, Vec<String>)> {
        Regex::new(pattern, false, false).unwrap().find(text, 0)
    }

    #[test]
    fn a_branch_reset_group_numbers_its_alternatives_alike() {
        let p = r#"<<(?|"([^"]+)"|'([^']+)'|(\w+))(;)?"#;
        assert_eq!(found(p, "<<'EOF'"), Some((0, 7, vec!["EOF".to_string()])));
        assert_eq!(
            found(p, "<<EOF;"),
            Some((0, 6, vec!["EOF".to_string(), ";".to_string()]))
        );
    }

    #[test]
    fn a_called_group_and_a_called_pattern_match_nested_text() {
        let p = r"\(((?:[^()]++|\((?1)\))*+)\)";
        assert_eq!(
            found(p, "(a(b(c))d) e"),
            Some((0, 10, vec!["a(b(c))d".to_string()]))
        );
        assert_eq!(
            found(r"\{(?:[^{}]|(?R))*\}", "{a{b}{c{d}}}x").map(|m| m.1),
            Some(12)
        );
    }
}
