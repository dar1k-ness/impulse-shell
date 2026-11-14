//! `~/.config/kde-material-you-colors/config.conf` ([CUSTOM]), read with configparser's rules
//! (case-insensitive keys, `=` or `:`, `#`/`;` comments, empty value = default).

use super::color::{self, Rgb};

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub icons_light: Option<String>,
    pub icons_dark: Option<String>,
    pub toolbar_opacity: f64,
    pub toolbar_opacity_dark: f64,
    pub konsole_opacity: f64,
    pub konsole_opacity_dark: f64,
    pub konsole_blur: bool,
    pub disable_konsole: bool,
    /// Only when set: the original reset Klassy/SierraBreeze titlebars to 100% on every switch.
    pub titlebar_opacity: Option<f64>,
    pub titlebar_opacity_dark: Option<f64>,
    pub custom_colors: Option<Vec<Rgb>>,
    pub chroma_mult: f64,
    pub tone_mult: f64,
    pub on_change_hook: Option<String>,
    pub sierra_breeze_buttons: bool,
    pub klassy_outline: bool,
    /// Space separated window classes for the KWin "darker titlebar" rule.
    pub darker_window_list: Option<String>,
    /// `pywal`: export the palette through pywal's templates and terminal sequences.
    pub pywal: bool,
    /// `pywal_light` (unless `pywal_follows_scheme`): pywal's and Konsole's mode, else Plasma's.
    pub pywal_light: Option<bool>,
}

fn custom(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut in_custom = false;
    for l in text.lines() {
        let t = l.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
            continue;
        }
        if t.starts_with('[') && t.ends_with(']') {
            in_custom = &t[1..t.len() - 1] == "CUSTOM";
            continue;
        }
        if !in_custom {
            continue;
        }
        let cut = t.find(['=', ':']);
        let (k, v) = match cut {
            Some(i) => (&t[..i], &t[i + 1..]),
            None => (t, ""),
        };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_string());
        match out.iter_mut().find(|(ok, _)| *ok == k) {
            Some(e) => e.1 = v,
            None => out.push((k, v)),
        }
    }
    out
}

fn boolean(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "yes" | "true" | "on" => Some(true),
        "0" | "no" | "false" | "off" => Some(false),
        _ => None,
    }
}

impl Options {
    pub fn parse(text: &str) -> Options {
        let kv = custom(text);
        let get = |k: &str| {
            kv.iter()
                .find(|(ok, _)| ok == k)
                .map(|(_, v)| v.as_str())
                .filter(|v| !v.is_empty())
        };
        let num = |k: &str| get(k).and_then(|v| v.parse::<f64>().ok());
        let pct = |k: &str| num(k).map(|v| v.clamp(0.0, 100.0));
        let flag = |k: &str| get(k).and_then(boolean).unwrap_or(false);
        let custom_colors = get("custom_colors_list").and_then(|v| {
            let c: Option<Vec<Rgb>> = v.split_whitespace().map(color::parse).collect();
            c.filter(|c| c.len() >= 7)
        });
        Options {
            icons_light: get("iconslight").map(str::to_string),
            icons_dark: get("iconsdark").map(str::to_string),
            toolbar_opacity: pct("toolbar_opacity").unwrap_or(100.0),
            toolbar_opacity_dark: pct("toolbar_opacity_dark").unwrap_or(100.0),
            konsole_opacity: pct("konsole_opacity").unwrap_or(100.0),
            konsole_opacity_dark: pct("konsole_opacity_dark").unwrap_or(100.0),
            konsole_blur: flag("konsole_blur"),
            disable_konsole: flag("disable_konsole"),
            titlebar_opacity: pct("titlebar_opacity"),
            titlebar_opacity_dark: pct("titlebar_opacity_dark"),
            custom_colors,
            chroma_mult: num("chroma_multiplier").unwrap_or(1.0),
            tone_mult: num("tone_multiplier").unwrap_or(1.0),
            on_change_hook: get("on_change_hook").map(str::to_string),
            sierra_breeze_buttons: flag("sierra_breeze_buttons_color"),
            klassy_outline: flag("klassy_windeco_outline"),
            darker_window_list: get("darker_window_list").map(str::to_string),
            pywal: flag("pywal"),
            pywal_light: get("pywal_light")
                .and_then(boolean)
                .filter(|_| !flag("pywal_follows_scheme")),
        }
    }

    /// Dark mode for pywal and Konsole when Plasma is in `dark`.
    pub fn pywal_dark(&self, dark: bool) -> bool {
        self.pywal_light.map_or(dark, |l| !l)
    }

    pub fn read(path: &std::path::Path) -> Options {
        Options::parse(&std::fs::read_to_string(path).unwrap_or_default())
    }
}
