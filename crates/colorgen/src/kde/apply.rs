//! Puts a [`Theme`] in place: the Plasma schemes, kdeglobals (what `plasma-apply-colorscheme` and
//! `plasma-changeicons` write, in one save), the KSyntaxHighlighting themes and Konsole, then
//! tells running Qt/KDE apps. Unlike the original it writes only what changed, skips the
//! `*2.colors` / `*_darker_titlebar*` copies (they only existed to defeat the "already applied"
//! check of `plasma-apply-colorscheme`), the unused `/tmp/kde-material-you-colors-$USER.json`
//! dump, and the KWin calls (BlendChanges, reconfigure) that fail on Hyprland.

use super::accent;
use super::color;
use super::config::Options;
use super::kconfig::{Doc, Edit, left, write_locked, written};
use super::theme::{Konsole, Theme};
use std::path::{Path, PathBuf};

/// Session-bus side effects, so tests can record them.
pub trait Bus {
    fn signal(&self, path: &str, iface: &str, member: &str, body: Body);
    fn has_owner(&self, name: &str) -> bool;
    fn call(&self, dest: &str, path: &str, iface: &str, member: &str);
    /// (service, session object path) of every running Konsole session.
    fn konsole_sessions(&self) -> Vec<(String, String)>;
    fn konsole_profile(&self, service: &str, session: &str) -> Option<String>;
    fn set_konsole_profile(&self, service: &str, session: &str, profile: &str);
    /// Open terminals (`/dev/pts/N`) pywal's sequences go to.
    fn ttys(&self) -> Vec<PathBuf> {
        crate::sys::Sys::ttys(&crate::live::Live)
    }
    fn write_tty(&self, tty: &Path, data: &[u8]) {
        crate::sys::Sys::write_tty(&crate::live::Live, tty, data);
    }
    /// pywal's `reload.env()` after the cache in `cache` was written.
    fn pywal_reload(&self, cache: &Path) {
        super::pywal::reload_env(cache);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Empty,
    Ints(Vec<i32>),
    /// `org.kde.kconfig.notify.ConfigChanged`: group -> keys.
    Changed(Vec<(String, Vec<String>)>),
}

#[derive(Clone, Debug)]
pub struct Dirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

impl Dirs {
    pub fn from_env(home: &Path) -> Dirs {
        let xdg = |k: &str, d: &str| match std::env::var(k) {
            Ok(v) if !v.is_empty() => PathBuf::from(v),
            _ => home.join(d),
        };
        Dirs {
            config: xdg("XDG_CONFIG_HOME", ".config"),
            data: xdg("XDG_DATA_HOME", ".local/share"),
            cache: xdg("XDG_CACHE_HOME", ".cache"),
        }
    }
}

const COLOR_GROUPS: [&str; 7] = [
    "Colors:View",
    "Colors:Window",
    "Colors:Button",
    "Colors:Selection",
    "Colors:Tooltip",
    "Colors:Complementary",
    "Colors:Header",
];
/// Alphabetical, the order KConfig itself writes them in.
pub(super) const COLOR_KEYS: [&str; 12] = [
    "BackgroundAlternate",
    "BackgroundNormal",
    "DecorationFocus",
    "DecorationHover",
    "ForegroundActive",
    "ForegroundInactive",
    "ForegroundLink",
    "ForegroundNegative",
    "ForegroundNeutral",
    "ForegroundNormal",
    "ForegroundPositive",
    "ForegroundVisited",
];
const WM_KEYS: [&str; 6] = [
    "activeBackground",
    "activeForeground",
    "inactiveBackground",
    "inactiveForeground",
    "activeBlend",
    "inactiveBlend",
];
const EFFECT_KEYS: [&str; 9] = [
    "Enable",
    "ChangeSelectionColor",
    "IntensityEffect",
    "IntensityAmount",
    "ColorEffect",
    "ColorAmount",
    "Color",
    "ContrastEffect",
    "ContrastAmount",
];
const ICON_GROUPS: i32 = 6;

/// KConfig's `QColor` form: `r,g,b`, `r,g,b,a` when translucent.
fn qcolor(v: &str) -> Option<String> {
    let h = v.trim();
    if let Some(x) = h.strip_prefix('#').filter(|x| x.len() == 8) {
        let a = u8::from_str_radix(&x[..2], 16).ok()?;
        let c = color::parse(h)?;
        return Some(if a == 255 {
            color::triple(c)
        } else {
            format!("{},{a}", color::triple(c))
        });
    }
    color::parse(h).map(color::triple)
}

#[derive(Default)]
pub(super) struct Notes(Vec<(String, Vec<String>)>);

impl Notes {
    pub(super) fn add(&mut self, group: &str, key: &str) {
        match self.0.iter_mut().find(|(g, _)| g == group) {
            Some((_, k)) => k.push(key.to_string()),
            None => self.0.push((group.to_string(), vec![key.to_string()])),
        }
    }
}

/// `applyScheme` (plasma-workspace `colorsapplicator.cpp`); the accent colour parts are in
/// [`accent`]. Returns whether any colour changed.
fn apply_scheme(kg: &mut Doc, scheme: &Doc, text: &str, name: &str, notes: &mut Notes) -> bool {
    let before = kg.bytes();
    let hash = sha1_smol::Sha1::from(text.as_bytes()).digest().to_string();
    kg.set("General", "ColorSchemeHash", &hash);
    let accent = accent::accent(kg).map(|a| accent::Options::new(scheme, a));
    if let Some(o) = &accent {
        for g in COLOR_GROUPS {
            kg.clear_group(g);
        }
        accent::color_sets(kg, scheme, &COLOR_GROUPS, o);
    }
    for g in COLOR_GROUPS.into_iter().filter(|_| accent.is_none()) {
        kg.clear_group(g);
        if g == "Colors:Header" && !scheme.has_group(g) {
            continue;
        }
        for k in COLOR_KEYS {
            if let Some(v) = scheme.get(g, k) {
                kg.set(g, k, v);
            }
        }
        let inactive = format!("{g}][Inactive");
        if scheme.has_group(&inactive) {
            let from = if g == "Colors:Header" {
                "Colors:Window".to_string()
            } else {
                inactive.clone()
            };
            for k in COLOR_KEYS {
                if let Some(v) = scheme.get(&from, k) {
                    kg.set(&inactive, k, v);
                }
            }
        }
    }
    for k in WM_KEYS {
        if let Some(v) = scheme.get("WM", k).and_then(qcolor) {
            kg.set("WM", k, &v);
        }
    }
    let notify = |kg: &mut Doc, notes: &mut Notes, g: &str, k: &str, v: &str| {
        if kg.set(g, k, v) {
            notes.add(g, k);
        }
    };
    notify(
        kg,
        notes,
        "KDE",
        "frameContrast",
        scheme.get("KDE", "frameContrast").unwrap_or("0.2"),
    );
    notify(
        kg,
        notes,
        "KDE",
        "contrast",
        scheme.get("KDE", "contrast").unwrap_or("7"),
    );
    if let Some(o) = &accent {
        accent::window_manager(kg, scheme, o, notes);
    }
    for g in ["ColorEffects:Inactive", "ColorEffects:Disabled"] {
        for k in EFFECT_KEYS {
            notify(kg, notes, g, k, scheme.get(g, k).unwrap_or(""));
        }
    }
    notify(kg, notes, "General", "ColorScheme", name);
    kg.bytes() != before
}

/// What `plasma-apply-colorscheme <name>` does to `kdeglobals` for the scheme `text`.
pub fn apply_scheme_text(kg: &mut Doc, text: &str, name: &str) -> bool {
    apply_scheme(kg, &Doc::parse(text), text, name, &mut Notes::default())
}

/// What [`apply`] did, for logs and tests.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub written: Vec<PathBuf>,
    pub icons: Option<String>,
    pub konsole_reloaded: usize,
}

/// `wallpaper`: the colour source as given, which pywal's palette records.
pub fn apply(
    dirs: &Dirs,
    theme: &Theme,
    o: &Options,
    dark: bool,
    wallpaper: &str,
    bus: &dyn Bus,
) -> Report {
    let mut r = Report::default();
    let put = |p: PathBuf, data: &str, r: &mut Report| {
        let w = written(&p, write_locked(&p, data.as_bytes()));
        if w {
            r.written.push(p);
        }
        w
    };
    let schemes = dirs.data.join("color-schemes");
    put(
        schemes.join("MaterialYouLight.colors"),
        &theme.plasma_light,
        &mut r,
    );
    put(
        schemes.join("MaterialYouDark.colors"),
        &theme.plasma_dark,
        &mut r,
    );

    let name = if dark {
        "MaterialYouDark"
    } else {
        "MaterialYouLight"
    };
    let kg_path = dirs.config.join("kdeglobals");
    match Doc::edit(&kg_path) {
        Ok(kg) => kdeglobals(kg, kg_path, dirs, theme, o, dark, name, bus, &mut r),
        Err(e) => left(&kg_path, &e),
    }

    let ks = dirs.data.join("org.kde.syntax-highlighting/themes");
    put(
        ks.join("material-you-dark.theme"),
        &theme.ksyntax_dark,
        &mut r,
    );
    put(
        ks.join("material-you-light.theme"),
        &theme.ksyntax_light,
        &mut r,
    );

    super::deco::apply(dirs, theme, o, dark, bus, &mut r);

    let wal_dark = o.pywal_dark(dark);
    if !o.disable_konsole {
        konsole(dirs, theme.konsole(wal_dark), o, wal_dark, bus, &mut r);
    }
    if o.pywal {
        super::pywal::apply(dirs, theme, wal_dark, wallpaper, bus, &mut r);
    }
    r
}

/// The scheme and the icon theme into `kdeglobals`, open under its lock, and the notifications
/// for what changed in it. Nothing is announced when the file could not be written.
#[allow(clippy::too_many_arguments)]
fn kdeglobals(
    mut kg: Edit,
    kg_path: PathBuf,
    dirs: &Dirs,
    theme: &Theme,
    o: &Options,
    dark: bool,
    name: &str,
    bus: &dyn Bus,
    r: &mut Report,
) {
    let mut notes = Notes::default();
    let text = theme.plasma(dark);
    let colors_changed = apply_scheme(&mut kg, &Doc::parse(text), text, name, &mut notes);
    let icons = if dark { &o.icons_dark } else { &o.icons_light };
    let mut icons_changed = None;
    if let Some(icons) = icons {
        let icons = icons.rsplit('/').next().unwrap_or(icons);
        if kg.set("Icons", "Theme", icons) {
            notes.add("Icons", "Theme");
            icons_changed = Some(icons.to_string());
        }
    }
    match kg.save() {
        Ok(true) => r.written.push(kg_path),
        Ok(false) => {}
        Err(e) => {
            left(&kg_path, &e);
            return;
        }
    }
    r.icons = icons_changed;
    if !notes.0.is_empty() {
        bus.signal(
            "/kdeglobals",
            "org.kde.kconfig.notify",
            "ConfigChanged",
            Body::Changed(notes.0),
        );
    }
    if colors_changed {
        bus.signal(
            "/KGlobalSettings",
            "org.kde.KGlobalSettings",
            "notifyChange",
            Body::Ints(vec![0, 0]),
        );
    }
    if r.icons.is_some() {
        let _ = std::fs::remove_file(dirs.cache.join("icon-cache.kcache"));
        for g in 0..ICON_GROUPS {
            bus.signal(
                "/KIconLoader",
                "org.kde.KIconLoader",
                "iconChanged",
                Body::Ints(vec![g]),
            );
        }
    }
}

const DEFAULT_PROFILE: &str = "Profile 1";

/// Python's `str(float)`.
fn py_float(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e16 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

fn konsole(dirs: &Dirs, k: &Konsole, o: &Options, dark: bool, bus: &dyn Bus, r: &mut Report) {
    let kdir = dirs.data.join("konsole");
    let scheme_path = kdir.join("MaterialYou.colorscheme");
    let mut d = match Doc::edit(&scheme_path) {
        Ok(d) => d,
        Err(e) => {
            left(&scheme_path, &e);
            return;
        }
    };
    let rgb = |c| color::triple(c);
    d.set("Background", "Color", &rgb(k.background));
    d.set("BackgroundIntense", "Color", &rgb(k.background_intense));
    d.set("BackgroundFaint", "Color", &rgb(k.background_faint));
    for i in 0..8 {
        d.set(&format!("Color{i}"), "Color", &rgb(k.normal[i]));
        d.set(&format!("Color{i}Intense"), "Color", &rgb(k.intense[i]));
        d.set(&format!("Color{i}Faint"), "Color", &rgb(k.faint[i]));
    }
    d.set("Foreground", "Color", &rgb(k.foreground));
    d.set("ForegroundIntense", "Color", &rgb(k.foreground_intense));
    d.set("ForegroundFaint", "Color", &rgb(k.foreground_faint));
    d.set("General", "Description", "MaterialYou");
    let opacity = if dark {
        o.konsole_opacity_dark
    } else {
        o.konsole_opacity
    } / 100.0;
    d.set("General", "Opacity", &py_float(opacity));
    d.set(
        "General",
        "Blur",
        if o.konsole_blur { "true" } else { "false" },
    );
    // The twin scheme is this one under another name, a file of ours whole.
    let mut twin = Doc::clone(&d);
    let mut changed = written(&scheme_path, d.save());
    if changed {
        r.written.push(scheme_path);
    }
    twin.set("General", "Description", "MaterialYouAlt");
    let alt = kdir.join("MaterialYouAlt.colorscheme");
    if written(&alt, write_locked(&alt, &twin.bytes())) {
        r.written.push(alt);
    }

    let rc_path = dirs.config.join("konsolerc");
    let mut rc = match Doc::edit(&rc_path) {
        Ok(rc) => rc,
        Err(e) => {
            // Which profile Konsole uses is in the file that cannot be read.
            left(&rc_path, &e);
            return;
        }
    };
    let profile = match rc
        .get("Desktop Entry", "DefaultProfile")
        .filter(|p| !p.is_empty())
    {
        Some(p) => p.strip_suffix(".profile").unwrap_or(p).to_string(),
        None => {
            rc.set(
                "Desktop Entry",
                "DefaultProfile",
                &format!("{DEFAULT_PROFILE}.profile"),
            );
            DEFAULT_PROFILE.to_string()
        }
    };
    if written(&rc_path, rc.save()) {
        r.written.push(rc_path);
    }
    let ppath = kdir.join(format!("{profile}.profile"));
    match Doc::edit(&ppath) {
        Ok(mut p) => {
            if p.is_new() {
                p.set("General", "Name", &profile);
                p.set("General", "Parent", "FALLBACK/");
            }
            p.set("Appearance", "ColorScheme", "MaterialYou");
            if written(&ppath, p.save()) {
                r.written.push(ppath);
                changed = true;
            }
        }
        Err(e) => left(&ppath, &e),
    }
    let mut t = Doc::default();
    t.set("Appearance", "ColorScheme", "MaterialYouAlt");
    t.set("General", "Name", "TempMyou");
    t.set("General", "Parent", &profile);
    let tpath = kdir.join("TempMyou.profile");
    if written(&tpath, write_locked(&tpath, &t.bytes())) {
        r.written.push(tpath);
    }
    if !changed {
        return;
    }
    // Konsole only re-reads a scheme when a session's profile changes: flip to the twin profile
    // (same colours under another scheme name) and back on the next switch.
    for (svc, s) in bus.konsole_sessions() {
        let cur = bus.konsole_profile(&svc, &s);
        let next = if cur.as_deref() == Some(profile.as_str()) {
            "TempMyou"
        } else {
            profile.as_str()
        };
        bus.set_konsole_profile(&svc, &s, next);
        r.konsole_reloaded += 1;
    }
}
