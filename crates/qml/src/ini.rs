//! INI-style key files read by the shell (icon theme `index.theme`, `kdeglobals`, KDE color
//! schemes, GTK `settings.ini`, `qt6ct.conf`) via `rust-ini`, read verbatim: no quote or
//! backslash processing, the first occurrence of a key in a section wins. KConfig nested
//! groups (`[Colors:Header][Inactive]`) keep their full bracketed name. Unlike KConfig,
//! rust-ini also splits `key: value` at `:`; none of the keys read here contain one.
use std::collections::HashMap;

fn parse(text: &str) -> Option<ini::Ini> {
    let opt = ini::ParseOption {
        enabled_quote: false,
        enabled_escape: false,
        enabled_indented_mutiline_value: false,
        enabled_preserve_key_leading_whitespace: false,
    };
    ini::Ini::load_from_str_opt(text, opt).ok()
}

/// Parsed documents of the last few texts: icon lookups re-read `kdeglobals` for the theme
/// name on every cache miss, and rust-ini is slower to parse than a scan for one key.
fn parsed(text: &str) -> Option<std::rc::Rc<ini::Ini>> {
    thread_local! {
        static RECENT: std::cell::RefCell<Vec<(String, Option<std::rc::Rc<ini::Ini>>)>> =
            Default::default();
    }
    RECENT.with(|r| {
        let mut r = r.borrow_mut();
        if let Some(i) = r.iter().position(|(t, _)| t == text) {
            let e = r.remove(i);
            let doc = e.1.clone();
            r.push(e);
            return doc;
        }
        let doc = parse(text).map(std::rc::Rc::new);
        if r.len() == 8 {
            r.remove(0);
        }
        r.push((text.to_string(), doc.clone()));
        doc
    })
}

/// `key` in the first `[section]` holding it.
pub(crate) fn value(text: &str, section: &str, key: &str) -> Option<String> {
    parsed(text)?
        .section_all((!section.is_empty()).then_some(section))
        .find_map(|p| p.get(key))
        .map(str::to_string)
}

/// All `key=value` pairs per section.
pub(crate) fn sections(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    let Some(ini) = parse(text) else {
        return out;
    };
    for (sec, props) in ini.iter() {
        let m = out.entry(sec.unwrap_or_default().to_string()).or_default();
        for (k, v) in props.iter() {
            m.entry(k.to_string()).or_insert_with(|| v.to_string());
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod legacy {
    //! The hand-written parser `ini` replaced, kept as the equivalence reference.
    use std::collections::HashMap;

    pub fn value(text: &str, section: &str, key: &str) -> Option<String> {
        let mut cur = "";
        for l in text.lines() {
            let l = l.trim();
            if let Some(s) = l.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                cur = s;
            } else if cur == section
                && let Some((k, v)) = l.split_once('=')
                && k.trim() == key
            {
                return Some(v.trim().to_string());
            }
        }
        None
    }

    pub fn sections(text: &str) -> HashMap<&str, HashMap<&str, &str>> {
        let mut out: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
        let mut cur = "";
        for l in text.lines() {
            let l = l.trim();
            if let Some(s) = l.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                cur = s;
            } else if let Some((k, v)) = l.split_once('=') {
                out.entry(cur)
                    .or_default()
                    .entry(k.trim())
                    .or_insert(v.trim());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Key files on this machine plus `RI_INI_CORPUS` (colon-separated directories, searched
    /// recursively for `index.theme`, `*.colors`, `*.ini`, `*.conf`, `*rc`, `kdeglobals`).
    pub(crate) fn corpus() -> Vec<std::path::PathBuf> {
        let mut roots: Vec<std::path::PathBuf> = ["/usr/share/icons", "/usr/share/color-schemes"]
            .iter()
            .map(Into::into)
            .collect();
        if let Ok(h) = std::env::var("HOME") {
            roots.push(std::path::Path::new(&h).join(".config"));
        }
        if let Ok(c) = std::env::var("RI_INI_CORPUS") {
            roots.extend(c.split(':').filter(|s| !s.is_empty()).map(Into::into));
        }
        let mut out = vec![];
        let mut stack: Vec<(std::path::PathBuf, u32)> = roots.into_iter().map(|r| (r, 0)).collect();
        while let Some((d, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                let n = e.file_name().to_string_lossy().into_owned();
                if p.is_dir() {
                    if depth < 8 {
                        stack.push((p, depth + 1));
                    }
                } else if n == "index.theme"
                    || n == "kdeglobals"
                    || n.ends_with("rc")
                    || [".colors", ".ini", ".conf"].iter().any(|x| n.ends_with(x))
                {
                    out.push(p);
                }
            }
        }
        out.sort();
        out
    }

    fn check(text: &str) -> Result<usize, String> {
        let old = legacy::sections(text);
        let new = sections(text);
        let mut n = 0;
        for (s, kv) in &old {
            for (k, v) in kv {
                if k.contains(':') {
                    continue;
                }
                n += 1;
                let got = new.get(*s).and_then(|m| m.get(*k)).map(String::as_str);
                if got != Some(*v) {
                    return Err(format!("[{s}] {k}: legacy {v:?}, rust-ini {got:?}"));
                }
                let one = value(text, s, k);
                if one.as_deref() != legacy::value(text, s, k).as_deref() {
                    return Err(format!("value([{s}] {k}): {one:?}"));
                }
            }
        }
        let lib: usize = new
            .iter()
            .map(|(s, m)| {
                let colon = old
                    .get(s.as_str())
                    .map_or(0, |o| o.keys().filter(|k| k.contains(':')).count());
                m.len().saturating_sub(colon)
            })
            .sum();
        if lib != n {
            return Err(format!("rust-ini has {lib} keys, legacy {n}"));
        }
        Ok(n)
    }

    #[test]
    fn matches_legacy_on_samples() {
        let samples = [
            "[Icon Theme]\nName=Breeze\nInherits=hicolor\nDirectories=16x16/apps,scalable/apps\n\n[16x16/apps]\nSize=16\nType=Fixed\n",
            "[General]\nfont=Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\nColorScheme=BreezeDark\n[Icons]\nTheme=Papirus-Dark\n",
            "[Colors:Window]\nBackgroundNormal=32,35,38\n[Colors:View]\nForegroundLink=29,153,243\n",
            "[Settings]\ngtk-icon-theme-name=\"Papirus\"\n  gtk-theme-name = adw-gtk3-dark \n",
            "[Appearance]\nicon_theme=breeze\n[SettingsWindow]\ngeometry=@ByteArray(\\x1\\xd9\\xd0\\xcb\\0\\x3)\n",
            "[KDE]\nLookAndFeelPackage=org.kde.breezedark.desktop\n[$Version]\nupdate_info=a.upd:x,b.upd:y\n",
            "# comment\n; other\n[A]\nk=1\nk=2\n[B]\nk=3\n[A]\nk=4\nj=5\n",
            "[Recent]\nRecent Files[$e]=file:///home/u/a.txt,$HOME/b\nName[ru]=Файлы\n",
            "[a]\nempty=\nspaced  =  v  w  \n",
            "top=1\n[Colors:Header][Inactive]\nBackgroundNormal=1,2,3\n[Colors:Header]\nBackgroundNormal=4,5,6\n",
        ];
        for s in samples {
            check(s).unwrap_or_else(|e| panic!("{e}\n---\n{s}"));
        }
    }

    #[test]
    fn matches_legacy_on_installed_key_files() {
        let files = corpus();
        let (mut keys, mut checked) = (0, 0);
        let mut fails = vec![];
        for f in &files {
            let Ok(t) = std::fs::read_to_string(f) else {
                continue;
            };
            checked += 1;
            match check(&t) {
                Ok(n) => keys += n,
                Err(e) => fails.push(format!("{}: {e}", f.display())),
            }
        }
        elog::eprintln!(
            "ini corpus: {checked} files, {keys} keys, {} differ",
            fails.len()
        );
        assert!(fails.is_empty(), "{}", fails.join("\n"));
    }
}
