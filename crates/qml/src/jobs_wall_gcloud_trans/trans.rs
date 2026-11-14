//! modules/rusty/sidebarLeft/Translator.qml: `TextTranslation` translates its `text` through
//! Google's endpoint the way translate-shell 0.9.7.1 does (`trans -brief -no-bidi -source S
//! -target T TEXT`), and `TextTranslationLanguages` lists the languages (`trans
//! -list-languages -no-bidi`). The text is text whatever it looks like (an option, a URL, a
//! `en:fr` shortcut), and no outside program runs.
use super::langs::{EXTRA_ALIASES, LOCALES, Locale};
use crate::jobs::Job;
use crate::runtime::World;
use std::collections::HashMap;
use std::sync::OnceLock;

const HOST: &str = "translate.googleapis.com";
const DEFAULT_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, \
                          like Gecko) Chrome/104.0.0.0 Safari/537.36 Edg/104.0.1293.54";

/// Aliases two locales share: gawk's hash order decides which one wins.
const AMBIGUOUS: &[&str] = &["iku", "inuktitut", "por"];

fn aliases() -> &'static HashMap<String, &'static str> {
    static A: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    A.get_or_init(|| {
        let mut m = HashMap::new();
        for l in LOCALES {
            m.insert(l.iso.to_string(), l.code);
            m.insert(l.name.to_lowercase(), l.code);
            if let Some(n) = l.name2 {
                m.insert(n.to_lowercase(), l.code);
            }
            m.insert(l.endonym.to_lowercase(), l.code);
            if let Some(e) = l.endonym2 {
                m.insert(e.to_lowercase(), l.code);
            }
        }
        for a in AMBIGUOUS {
            m.remove(*a);
        }
        for (a, c) in EXTRA_ALIASES {
            m.insert(a.to_string(), c);
        }
        m
    })
}

fn locale(code: &str) -> Option<&'static Locale> {
    LOCALES.iter().find(|l| l.code == code)
}

/// `getCode`: a locale key for a code, alias, name or endonym; None if trans finds none (or
/// only an ambiguous alias).
pub(super) fn get_code(code: &str) -> Option<String> {
    if code == "auto" || locale(code).is_some() {
        return Some(code.to_string());
    }
    if AMBIGUOUS.contains(&code) || AMBIGUOUS.contains(&code.to_lowercase().as_str()) {
        return None;
    }
    let a = aliases();
    if let Some(c) = a.get(code).or_else(|| a.get(&code.to_lowercase())) {
        return Some(c.to_string());
    }
    // `^([[:alpha:]][[:alpha:]][[:alpha:]]?)-(.*)$`: drop an unknown region/script part.
    let (head, _) = code.split_once('-')?;
    let n = head.chars().count();
    ((2..=3).contains(&n) && head.chars().all(char::is_alphabetic)).then(|| head.to_string())
}

fn is_rtl(code: &str) -> bool {
    get_code(code)
        .and_then(|c| locale(&c))
        .is_some_and(|l| l.rtl)
}

/// `parseLang` on the user's POSIX locale.
fn parse_lang(lang: &str) -> String {
    let b = lang.as_bytes();
    let n = b.iter().take_while(|c| c.is_ascii_lowercase()).count();
    let mut code = if (2..=3).contains(&n) && (b.len() == n || b[n] == b'_') {
        get_code(&lang[..n])
    } else {
        None
    };
    if lang.starts_with("zh_CN") || lang.starts_with("zh_SG") {
        code = Some("zh-CN".into());
    } else if lang.starts_with("zh_TW") || lang.starts_with("zh_HK") {
        code = Some("zh-TW".into());
    }
    code.unwrap_or_else(|| "en".into())
}

/// The process environment by name: where requests look for a proxy.
type Vars = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// What a translation takes from the shell's environment.
pub(super) struct Env {
    pub hl: String,
    pub ua: String,
    pub fribidi: bool,
    pub base: String,
    pub vars: Vars,
}

impl Env {
    fn from_env() -> Env {
        let e = super::env;
        let locale = [e("LC_ALL"), e("LANG")]
            .into_iter()
            .find(|l| !l.is_empty())
            .unwrap_or_else(|| "en_US.UTF-8".into());
        let hl = [e("HOST_LANG"), e("HOME_LANG")]
            .into_iter()
            .find(|l| !l.is_empty())
            .unwrap_or_else(|| parse_lang(&locale));
        let ua = Some(e("USER_AGENT"))
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| DEFAULT_UA.into());
        Env {
            hl,
            ua,
            fribidi: crate::osutil::on_path("fribidi"),
            base: format!("http://{HOST}"),
            vars: Box::new(|k| std::env::var(k).ok()),
        }
    }
}

/// JS `String.prototype.trim`.
pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

/// One translation: `text` from each of the languages `sls` to each of `tls`.
pub(super) struct Plan {
    sls: Vec<String>,
    tls: Vec<String>,
    text: String,
}

/// The translation of `text` from `sl` to `tl`, each one language or several joined by `+`
/// (a code, name or endonym; no `sl` is `auto`). `Err` is the message for a language that
/// names none, or for no target at all.
pub(super) fn plan(sl: &str, tl: &str, text: &str) -> Result<Plan, String> {
    let langs = |list: &str| {
        list.split('+')
            .filter(|c| !c.is_empty())
            .map(|c| match get_code(c) {
                Some(_) => Ok(c.to_string()),
                None => Err(format!("[ERROR] Unknown language code: {c}")),
            })
            .collect::<Result<Vec<_>, _>>()
    };
    let sls = langs(if sl.is_empty() { "auto" } else { sl })?;
    let tls = langs(tl)?;
    if sls.is_empty() || tls.is_empty() {
        return Err("[ERROR] No language to translate to or from".into());
    }
    Ok(Plan {
        sls,
        tls,
        text: text.to_string(),
    })
}

pub static TRANSLATE: crate::impulse::Element = crate::impulse::Element {
    job: Some(translate),
    ..crate::impulse::Element::new("TextTranslation")
};

/// `Impulse/TextTranslation`: `text` (trimmed) from language `source` to language `target`, the
/// translation on stdout. A language it does not know ends the run with code 1 and the
/// message on stderr.
fn translate(w: &mut World, p: usize) -> Option<Job> {
    let sl = crate::jobs::text(w, p, "source");
    let tl = crate::jobs::text(w, p, "target");
    let text = crate::jobs::text(w, p, "text");
    let plan = plan(&sl, &tl, js_trim(&text));
    let env = Env::from_env();
    Some(Box::new(move |io, _| match &plan {
        Ok(plan) => run(plan, &env, &|s| io.out(s), &|s| io.err(s)),
        Err(e) => {
            error(&|s| io.err(s), e);
            1
        }
    }))
}

pub static LANGUAGES: crate::impulse::Element = crate::impulse::Element {
    job: Some(list_languages),
    ..crate::impulse::Element::new("TextTranslationLanguages")
};

/// `Impulse/TextTranslationLanguages`: the languages [`translate`] knows, one name per line.
fn list_languages(_: &mut World, _: usize) -> Option<Job> {
    Some(Box::new(|io, _| {
        io.out(languages());
        0
    }))
}

pub(super) fn languages() -> String {
    let mut l: Vec<&Locale> = LOCALES
        .iter()
        .filter(|l| !l.supported_by.is_empty())
        .collect();
    l.sort_by(|a, b| a.code.as_bytes().cmp(b.code.as_bytes()));
    l.iter().map(|l| format!("{}\n", l.endonym)).collect()
}

fn warn(err: &dyn Fn(String), t: &str) {
    err(format!("\x1b[33m{t}\x1b[0m\n"));
}

fn error(err: &dyn Fn(String), t: &str) {
    err(format!("\x1b[1m\x1b[33m{t}\x1b[0m\x1b[22m\n"));
}

fn rtl_warn(err: &dyn Fn(String), env: &Env, code: &str) {
    if is_rtl(code) && !env.fribidi {
        let name = get_code(code)
            .and_then(|c| locale(&c))
            .map_or("", |l| l.name);
        warn(
            err,
            &format!("[WARNING] {name} is a right-to-left language, but FriBidi is not found."),
        );
    }
}

/// Main.awk's run over all source and target languages: stdout, stderr and exit code.
pub(super) fn run(plan: &Plan, env: &Env, out: &dyn Fn(String), err: &dyn Fn(String)) -> i32 {
    // No text: nothing to ask for.
    if plan.text.is_empty() {
        return 0;
    }
    let mut hl = env.hl.clone();
    let mut code = 0;
    for sl in &plan.sls {
        for tl in &plan.tls {
            if get_code(&hl).is_none() {
                warn(
                    err,
                    &format!("[WARNING] Unknown language code: {hl}, fallback to English: en"),
                );
                hl = "en".into();
            } else {
                rtl_warn(err, env, &hl);
            }
            rtl_warn(err, env, sl);
            rtl_warn(err, env, tl);
            match google_brief(&plan.text, sl, tl, &hl, env, err) {
                Ok(Some(t)) => out(t + "\n"),
                Ok(None) => {
                    code = 1;
                    out("\n".into());
                }
                Err(fatal) => {
                    err(fatal);
                    return 2;
                }
            }
        }
    }
    code
}

/// `googleRequestUrl`'s path: every byte of the text percent-encoded (`dumpX` pipes `echo TEXT`
/// through hexdump, but awk's `split(…, " ")` drops the newline's trailing field).
pub(super) fn request_path(text: &str, sl: &str, tl: &str, hl: &str) -> String {
    let mut q = String::with_capacity(text.len() * 3 + 3);
    for b in text.bytes() {
        q.push_str(&format!("%{b:02X}"));
    }
    format!(
        "/translate_a/single?client=gtx&ie=UTF-8&oe=UTF-8&dt=bd&dt=ex&dt=ld&dt=md&dt=rw&dt=rm\
         &dt=ss&dt=t&dt=at&dt=gt&dt=qca&sl={sl}&tl={tl}&hl={hl}&q={q}"
    )
}

/// `googleTranslate` in brief mode: Some(translation), None for the "Oops" error, Err for
/// gawk's fatal error when the server cannot be reached.
fn google_brief(
    text: &str,
    sl: &str,
    tl: &str,
    hl: &str,
    env: &Env,
    err: &dyn Fn(String),
) -> Result<Option<String>, String> {
    let code = |c: &str| get_code(c).unwrap_or_else(|| c.to_string());
    let url = format!(
        "{}{}",
        env.base,
        request_path(text, &code(sl), &code(tl), &code(hl))
    );
    // Through the shell's HTTP client: the proxy the environment names, and the run's stop.
    let headers = [
        ("Connection".to_string(), "close".to_string()),
        ("User-Agent".to_string(), env.ua.clone()),
    ];
    let get = |o| crate::http::send_env("GET", &url, &headers, None, o, &*env.vars);
    let res = match get(crate::http::Opts {
        answer_within: Some(std::time::Duration::from_secs(2)),
        ..Default::default()
    }) {
        Err(crate::http::Error::Timeout(_)) => {
            warn(
                err,
                "[WARNING] Connection timed out. Retrying IPv4 connection.",
            );
            get(crate::http::Opts {
                ipv4: true,
                ..Default::default()
            })
        }
        r => r,
    };
    let res = res.map_err(|e| {
        format!(
            "gawk: fatal: cannot open two way pipe `/inet/tcp/0/{HOST}/80' for input/output: {e}\n"
        )
    })?;
    let status = res.status;
    let location = res
        .header("location")
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "));
    let chunked = res
        .header("transfer-encoding")
        .is_some_and(|t| t.contains("chunked"));
    let body = res.bytes_max(u64::MAX).unwrap_or_default();
    let body = String::from_utf8_lossy(&body);
    // gawk reads the raw HTTP/1.1 body line by line, chunk framing included (Google sends
    // one chunk).
    let raw = match (chunked, body.is_empty()) {
        (true, false) => format!("{:x}\r\n{body}\r\n0\r\n\r\n", body.len()),
        (true, true) => "0\r\n\r\n".to_string(),
        (false, _) => body.into_owned(),
    };
    let mut content = join_lines(&raw);
    if matches!(status, 301 | 302) && location.as_ref().is_some_and(|l| !l.is_empty()) {
        content = curl(location.as_deref().unwrap_or_default(), &env.ua);
    } else if status == 429 {
        error(
            err,
            "[ERROR] Google did not return results because rate limiting is in effect",
        );
        error(err, "[ERROR] Rate limiting");
    } else if status >= 400 {
        error(
            err,
            &format!("[ERROR] Google returned an error response. HTTP status code: {status}"),
        );
        error(err, "[ERROR] Other HTTP error");
    }
    // `assert(content, "[ERROR] Null response.")`
    let content = if content.truthy() {
        content.text
    } else {
        error(err, "[ERROR] Null response.");
        String::new()
    };
    let ast = parse_json_array(&tokenize(&content));
    if !ast.values().any(|v| awk_true(v, true)) {
        error(
            err,
            "[ERROR] Oops! Something went wrong and I can't translate it for you :(",
        );
        return Ok(None);
    }
    let mut keys: Vec<&Vec<i64>> = ast.keys().collect();
    keys.sort();
    let mut translation = String::new();
    for k in keys {
        let v = &ast[k];
        if v == "null" {
            continue;
        }
        match k.as_slice() {
            [0, 0, _, 0] => translation.push_str(&literal(v)),
            [0, 7, 5] => {
                if v == "true" {
                    warn(
                        err,
                        "Showing translation for:  (use -no-auto to disable autocorrect)",
                    );
                } else {
                    let m = ast.get(&vec![0, 7, 1]).map_or("", |s| s.as_str());
                    warn(
                        err,
                        &format!("Did you mean: \x1b[1m{}\x1b[22m", unparameterize(m)),
                    );
                }
            }
            _ => {}
        }
    }
    Ok(Some(translation))
}

/// Utils.awk `curl`: `curl --location --silent --user-agent UA URL`, lines joined.
fn curl(url: &str, ua: &str) -> Joined {
    let h = [("User-Agent".to_string(), ua.to_string())];
    let o = crate::http::Opts {
        follow: true,
        ..Default::default()
    };
    let t = crate::http::get(url, &h, o)
        .and_then(|r| Ok(r.text()?))
        .unwrap_or_default();
    join_lines(&t)
}

/// An awk string built from input lines: `text`, and whether it is still a bare input
/// record (a "strnum", false when it reads as the number 0).
struct Joined {
    text: String,
    strnum: bool,
}

impl Joined {
    fn truthy(&self) -> bool {
        awk_true(&self.text, self.strnum)
    }
}

/// `while (getline) content = content ? content "\n" $0 : $0` over `raw`'s lines.
fn join_lines(raw: &str) -> Joined {
    let mut c = Joined {
        text: String::new(),
        strnum: false,
    };
    let raw = raw.strip_suffix('\n').unwrap_or(raw);
    if raw.is_empty() {
        return c;
    }
    for l in raw.split('\n') {
        if c.truthy() {
            c.text.push('\n');
            c.text.push_str(l);
            c.strnum = false;
        } else {
            c.text = l.to_string();
            c.strnum = true;
        }
    }
    c
}

/// awk truthiness: empty is false, and so is input that looks like the number 0.
fn awk_true(s: &str, strnum: bool) -> bool {
    let t = s.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
    let cs: Vec<char> = t.chars().collect();
    match t.parse::<f64>() {
        Ok(n) if strnum && !cs.is_empty() && number_len(&cs) == Some(cs.len()) => n != 0.0,
        _ => !s.is_empty(),
    }
}

/// Parser.awk `tokenize` with its default delimiters, comments, operators and patterns.
pub(super) fn tokenize(s: &str) -> Vec<String> {
    let cs: Vec<char> = s.chars().collect();
    let starts = |i: usize, p: &str| {
        p.chars()
            .enumerate()
            .all(|(j, c)| cs.get(i + j) == Some(&c))
    };
    let mut toks = vec![];
    let mut cur = String::new();
    let (mut quoting, mut escaping, mut block, mut line) = (false, false, false, false);
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if block {
            if ["|#", "*/", "*)"].iter().any(|p| starts(i, p)) {
                block = false;
            }
            i += 1;
        } else if line {
            if c == '\n' || c == '\r' {
                line = false;
            }
            i += 1;
        } else if quoting {
            cur.push(c);
            if escaping {
                escaping = false;
            } else if c == '"' {
                toks.push(std::mem::take(&mut cur));
                quoting = false;
            } else if c == '\\' {
                escaping = true;
            }
            i += 1;
        } else if matches!(c, ' ' | '\t' | '\u{b}' | '\n' | '\r') {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            i += 1;
        } else if c == '"' {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            cur.push(c);
            quoting = true;
            i += 1;
        } else if let Some(p) = ["#|", "/*", "(*"].iter().find(|p| starts(i, p)) {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            block = true;
            i += p.len();
        } else if let Some(p) = [";", "//", "#"].iter().find(|p| starts(i, p)) {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            line = true;
            i += p.len();
        } else if matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | ',') {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            toks.push(c.to_string());
            i += 1;
        } else if let Some(n) = number_len(&cs[i..]) {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            toks.push(cs[i..i + n].iter().collect());
            i += n;
        } else {
            cur.push(c);
            i += 1;
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    toks
}

/// The longest match of `[+-]?((0|[1-9][0-9]*)|[.][0-9]*|(0|[1-9][0-9]*)[.][0-9]*)
/// ([Ee][+-]?[0-9]+)?` at the start of `s`.
fn number_len(s: &[char]) -> Option<usize> {
    let digits = |from: usize| s[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let mut i = usize::from(matches!(s.first(), Some('+' | '-')));
    match s.get(i) {
        Some('0') => i += 1,
        Some('1'..='9') => i += digits(i),
        Some('.') => {}
        _ => return None,
    }
    if s.get(i) == Some(&'.') {
        i += 1;
        i += digits(i);
    }
    if matches!(s.get(i), Some('e' | 'E')) {
        let j = i + 1 + usize::from(matches!(s.get(i + 1), Some('+' | '-')));
        let d = digits(j);
        if d > 0 {
            i = j + d;
        }
    }
    Some(i)
}

/// Parser.awk `parseJsonArray`: every leaf token under its index path.
pub(super) fn parse_json_array(toks: &[String]) -> HashMap<Vec<i64>, String> {
    let mut ast = HashMap::new();
    let mut stack: HashMap<i64, i64> = HashMap::new();
    let mut p = 0i64;
    for t in toks {
        match t.as_str() {
            "(" | "[" | "{" => {
                p += 1;
                stack.insert(p, 0);
            }
            ")" | "]" | "}" => p -= 1,
            "," => *stack.entry(p).or_default() += 1,
            _ => {
                let key = (0..=p.max(0))
                    .filter(|&j| j == 0 || j <= p)
                    .map(|j| stack.get(&j).copied().unwrap_or(0))
                    .collect();
                ast.insert(key, t.clone());
            }
        }
    }
    ast
}

fn escape_char(c: &str) -> &str {
    match c {
        "b" => "\u{8}",
        "f" => "\u{c}",
        "n" => "\n",
        "r" => "\r",
        "t" => "\t",
        "v" => "\u{b}",
        "u0026" => "&",
        "u003c" => "<",
        "u003d" => "=",
        "u003e" => ">",
        "u200b" => "",
        c => c,
    }
}

/// Commons.awk `literal`: a quoted token's text with trans's (partial) escape handling.
pub(super) fn literal(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    if cs.len() < 2 || cs[0] != '"' || cs[cs.len() - 1] != '"' {
        return s.to_string();
    }
    let mut o = String::new();
    let mut escaping = false;
    let mut cc = String::new();
    for &c in &cs[1..cs.len() - 1] {
        if escaping {
            if !cc.is_empty() {
                cc.push(c);
                if cc.chars().count() == 5 {
                    o.push_str(escape_char(&cc));
                    escaping = false;
                    cc.clear();
                }
            } else if c == 'u' {
                cc.push(c);
            } else {
                o.push_str(escape_char(&c.to_string()));
                escaping = false;
            }
        } else if c == '\\' {
            escaping = true;
        } else {
            o.push(c);
        }
    }
    o
}

fn unparameterize(s: &str) -> String {
    if let Some(t) = s.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')) {
        return t.replace("'\\''", "'");
    }
    if let Some(t) = s.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
        return t.replace("\\\"", "\"").replace("\\\\", "\\");
    }
    s.to_string()
}

#[cfg(test)]
mod tests;
