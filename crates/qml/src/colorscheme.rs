//! QStyleHints::colorScheme behind `Material.System`: the colour scheme of the platform theme Qt
//! loads (QGenericUnixTheme::themeNames / createUnixTheme), not the XDG portal directly. The
//! generic theme has none (Unknown), which Material treats as Light.

use std::io::SeekFrom;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Changes the watcher saw (kdeglobals rewritten, a portal setting changed) and the latest of
/// them the scene was told about (`poll`). Values below are recomputed once per applied change.
static SEEN: AtomicU64 = AtomicU64::new(0);
static APPLIED: AtomicU64 = AtomicU64::new(0);

/// Bumps on each applied settings change: what the scene compares to drop styled draws/images.
pub fn generation() -> u64 {
    APPLIED.load(Ordering::Acquire)
}

/// A value derived from the system settings, recomputed after each applied change.
struct Live<T>(Mutex<Option<(u64, T)>>);

impl<T: Clone> Live<T> {
    const fn new() -> Self {
        Live(Mutex::new(None))
    }

    fn get(&self, compute: impl FnOnce() -> T) -> T {
        let g = generation();
        let mut c = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match &*c {
            Some((at, v)) if *at == g => v.clone(),
            _ => {
                let v = compute();
                *c = Some((g, v.clone()));
                v
            }
        }
    }
}

/// The portal's `org.freedesktop.appearance` `color-scheme` (0 no preference, 1 dark, 2 light);
/// None until the portal answered.
static PORTAL_SCHEME: Mutex<Option<u32>> = Mutex::new(None);
static WAKE: Mutex<Option<crate::svc::Handle>> = Mutex::new(None);

fn settings_changed() {
    SEEN.fetch_add(1, Ordering::AcqRel);
    if let Some(h) = WAKE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        h.wake();
    }
}

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const APPEARANCE: &str = "org.freedesktop.appearance";

/// Which change notifications the platform theme reacts to, as Qt's do: KConfigWatcher's
/// `org.kde.kconfig.notify.ConfigChanged` on `/kdeglobals` (plasma-integration; QKdeTheme
/// reads the same file) and the portal's `Settings.SettingChanged` (QGnomeTheme,
/// QXdgDesktopPortalTheme, QGtk3Theme).
fn sources(t: PlatformTheme) -> (bool, bool) {
    match t {
        PlatformTheme::KdePlugin | PlatformTheme::Kde => (true, false),
        PlatformTheme::Gnome | PlatformTheme::XdgPortal | PlatformTheme::Gtk3 => (false, true),
        PlatformTheme::Qt6ct | PlatformTheme::Generic => (false, false),
    }
}

/// Subscribes once to the theme's change signals and, for the portal themes, asks the portal
/// for the current scheme, all off the calling thread: a D-Bus-activated portal may take
/// seconds to answer, and the scene renders Light until it does (QStyleHints::colorScheme is
/// Unknown until then in Qt too).
fn watch() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let (kconfig, portal) = sources(platform_theme());
        if !kconfig && !portal {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("qs-colorscheme".into())
            .spawn(move || {
                let mut rules = vec![];
                if kconfig {
                    rules.extend(
                        zbus::MatchRule::builder()
                            .msg_type(zbus::message::Type::Signal)
                            .interface("org.kde.kconfig.notify")
                            .and_then(|r| r.member("ConfigChanged"))
                            .and_then(|r| r.path("/kdeglobals"))
                            .map(|r| r.build()),
                    );
                }
                if portal {
                    rules.extend(
                        zbus::MatchRule::builder()
                            .msg_type(zbus::message::Type::Signal)
                            .sender(PORTAL)
                            .and_then(|r| r.interface(SETTINGS))
                            .and_then(|r| r.member("SettingChanged"))
                            .and_then(|r| r.path(PORTAL_PATH))
                            .map(|r| r.build()),
                    );
                }
                let sink: crate::dbus::Sink = Box::new(|m| {
                    on_signal(m);
                    true
                });
                // Subscribed before the read, so a change racing it is not lost.
                if crate::dbus::subscribe_to(crate::dbus::Bus::Session, rules, sink).is_none() {
                    return;
                }
                if portal {
                    let scheme =
                        crate::dbus::session().and_then(|c| match read_portal_scheme(&c) {
                            Ok(v) => Some(v),
                            Err(e) => {
                                tracing::debug!("portal color-scheme: {e}");
                                None
                            }
                        });
                    let mut cur = PORTAL_SCHEME.lock().unwrap_or_else(|e| e.into_inner());
                    // A SettingChanged that arrived meanwhile is newer than this answer.
                    if cur.is_none() {
                        *cur = Some(scheme.unwrap_or(0));
                        drop(cur);
                        settings_changed();
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("colorscheme watcher: {e}");
        }
    });
}

fn on_signal(m: &zbus::Message) {
    let h = m.header();
    match h.member().map(|n| n.as_str()) {
        Some("ConfigChanged") => settings_changed(),
        Some("SettingChanged") => {
            let body = m.body();
            let Ok((ns, key, v)) =
                body.deserialize::<(String, String, zbus::zvariant::Value<'_>)>()
            else {
                return;
            };
            if ns == APPEARANCE && key == "color-scheme" {
                *PORTAL_SCHEME.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(scheme_value(&v).unwrap_or(0));
                settings_changed();
            } else if ns == "org.gnome.desktop.interface" {
                // QGtk3Theme follows gtk-theme / font changes through the same signal.
                settings_changed();
            }
        }
        _ => {}
    }
}

/// The `u` of a `color-scheme` setting, through the extra variant layer of Settings v1 `Read`.
fn scheme_value(v: &zbus::zvariant::Value<'_>) -> Option<u32> {
    match v {
        zbus::zvariant::Value::U32(n) => Some(*n),
        zbus::zvariant::Value::Value(inner) => scheme_value(inner),
        _ => None,
    }
}

/// Settings `ReadOne` (version 2), else the deprecated `Read` of version 1 portals, whose value
/// is wrapped in one more variant. A portal without the key reports no preference.
fn read_portal_scheme(c: &zbus::blocking::Connection) -> zbus::Result<u32> {
    let call = |method: &str| {
        c.call_method(
            Some(PORTAL),
            PORTAL_PATH,
            Some(SETTINGS),
            method,
            &(APPEARANCE, "color-scheme"),
        )
    };
    let m = match call("ReadOne") {
        Err(zbus::Error::MethodError(name, ..))
            if name.as_str() == "org.freedesktop.DBus.Error.UnknownMethod" =>
        {
            call("Read")
        }
        r => r,
    };
    let m = match m {
        Err(zbus::Error::MethodError(name, ..))
            if name.as_str() == "org.freedesktop.portal.Error.NotFound" =>
        {
            return Ok(0);
        }
        r => r?,
    };
    let body = m.body();
    let v: zbus::zvariant::Value<'_> = body.deserialize()?;
    Ok(scheme_value(&v).unwrap_or(0))
}

/// Applies a settings change the watcher saw: like Qt's QEvent::ThemeChange and
/// ApplicationFontChange it reaches every item, so texts re-measure with the new system font
/// and every window repaints with the new scheme, link color and icon colors.
pub fn poll(w: &mut crate::runtime::World) -> bool {
    {
        let mut wake = WAKE.lock().unwrap_or_else(|e| e.into_inner());
        if wake.is_none() {
            *wake = Some(w.waker.handle(crate::svc::Svc::Theme));
        }
    }
    watch();
    let seen = SEEN.load(Ordering::Acquire);
    if APPLIED.swap(seen, Ordering::AcqRel) == seen {
        return false;
    }
    if w.text_measure.is_some() {
        for ty in ["Text", "TextMetrics", "FontMetrics"] {
            for i in w.of_type(ty) {
                w.text_dirty.insert(i);
            }
        }
    }
    let live: Vec<usize> = (0..w.items.len()).filter(|i| !w.dead.contains(i)).collect();
    w.touched.extend(live);
    w.memo_m.clear();
    w.memo_p.clear();
    w.revision += 1;
    true
}

/// The platform theme QGuiApplicationPrivate::createPlatformTheme loads: over the names in
/// QT_QPA_PLATFORMTHEME then QGenericUnixTheme::themeNames (desktop-derived, then "generic"),
/// the first installed plugin, else the first built-in theme (kde needs KDE_SESSION_VERSION).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlatformTheme {
    /// plasma-integration's KDEPlasmaPlatformTheme6 plugin.
    KdePlugin,
    /// QKdeTheme (built into QtGui) on a KDE desktop without the plugin.
    Kde,
    Gtk3,
    /// QGnomeTheme (built into QtGui) on a GTK desktop without the gtk3 plugin.
    Gnome,
    Qt6ct,
    XdgPortal,
    Generic,
}

/// Whether a platformthemes plugin declares `key`: QFactoryLoader matches the plugin's metadata
/// `Keys` (case-insensitively), not its file name (plasma-integration ships
/// `KDEPlasmaPlatformTheme6.so`, Qt's own are `libq*.so`).
fn installed(key: &str) -> bool {
    static KEYS: OnceLock<Vec<String>> = OnceLock::new();
    KEYS.get_or_init(plugin_keys)
        .iter()
        .any(|k| k.eq_ignore_ascii_case(key))
}

fn plugin_keys() -> Vec<String> {
    let env = std::env::var("QT_PLUGIN_PATH").unwrap_or_default();
    env.split(':')
        .filter(|d| !d.is_empty())
        .chain(["/usr/lib/qt6/plugins", "/usr/lib64/qt6/plugins"])
        .filter_map(|d| std::fs::read_dir(Path::new(d).join("platformthemes")).ok())
        .flat_map(|rd| rd.flatten())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".so"))
        .filter_map(|e| std::fs::File::open(e.path()).ok())
        .filter_map(|mut f| qt_metadata(&mut f))
        .flat_map(|m| metadata_keys(&m))
        .collect()
}

/// Upper bound on the `.note.qt.metadata` section a plugin may declare.
const MAX_NOTE: u64 = 1 << 20;

/// The CBOR metadata of a Qt 6 plugin as QPluginLoader finds it (qelfparser.cpp): the
/// `qt-project!` note of type 0x74510001 in the SHT_NOTE section `.note.qt.metadata`, after the
/// 4-byte QPluginMetaData::Header. Reads only the ELF header, the section headers, their name
/// table and that section.
fn qt_metadata<F: std::io::Read + std::io::Seek>(f: &mut F) -> Option<Vec<u8>> {
    const SHT_NOTE: u32 = 7;
    let mut eh = [0u8; 64];
    f.read_exact(&mut eh[..52]).ok()?;
    if &eh[..4] != b"\x7fELF" {
        return None;
    }
    let wide = match eh[4] {
        1 => false,
        2 => true,
        _ => return None,
    };
    let be = match eh[5] {
        1 => false,
        2 => true,
        _ => return None,
    };
    if wide {
        f.read_exact(&mut eh[52..]).ok()?;
    }
    let rd = Endian(be);
    let (shoff, shentsize, shnum, shstrndx) = if wide {
        (
            rd.u64(&eh[40..]),
            rd.u16(&eh[58..]),
            rd.u16(&eh[60..]),
            rd.u16(&eh[62..]),
        )
    } else {
        (
            u64::from(rd.u32(&eh[32..])),
            rd.u16(&eh[46..]),
            rd.u16(&eh[48..]),
            rd.u16(&eh[50..]),
        )
    };
    let entsize = usize::from(shentsize);
    if entsize < if wide { 64 } else { 40 } || shstrndx >= shnum {
        return None;
    }
    let mut table = vec![0u8; entsize * usize::from(shnum)];
    f.seek(SeekFrom::Start(shoff)).ok()?;
    f.read_exact(&mut table).ok()?;
    // (name offset, type, file offset, size)
    let section = |k: usize| {
        let h = &table[k * entsize..];
        if wide {
            (
                rd.u32(h),
                rd.u32(&h[4..]),
                rd.u64(&h[24..]),
                rd.u64(&h[32..]),
            )
        } else {
            (
                rd.u32(h),
                rd.u32(&h[4..]),
                u64::from(rd.u32(&h[16..])),
                u64::from(rd.u32(&h[20..])),
            )
        }
    };
    let mut read = |off: u64, len: u64| {
        if len > MAX_NOTE {
            return None;
        }
        let mut b = vec![0u8; usize::try_from(len).ok()?];
        f.seek(SeekFrom::Start(off)).ok()?;
        f.read_exact(&mut b).ok()?;
        Some(b)
    };
    let (_, _, str_off, str_len) = section(usize::from(shstrndx));
    let names = read(str_off, str_len)?;
    const NAME: &[u8] = b".note.qt.metadata\0";
    let (_, _, off, len) = (0..usize::from(shnum))
        .map(section)
        .find(|&(name, ty, ..)| {
            ty == SHT_NOTE
                && names
                    .get(name as usize..)
                    .is_some_and(|n| n.starts_with(NAME))
        })?;
    let note = read(off, len)?;
    qt_note(&note, rd)
}

/// The CBOR payload of the Qt metadata note among the ELF notes `b` (Elf_Nhdr records, name and
/// descriptor each padded to 4 bytes).
fn qt_note(mut b: &[u8], rd: Endian) -> Option<Vec<u8>> {
    const NAME: &[u8] = b"qt-project!\0";
    const TYPE: u32 = 0x7451_0001;
    let pad = |n: usize| n.checked_add(3).map(|n| n & !3);
    while b.len() >= 12 {
        let (namesz, descsz, ty) = (
            rd.u32(b) as usize,
            rd.u32(&b[4..]) as usize,
            rd.u32(&b[8..]),
        );
        let name_end = 12usize.checked_add(pad(namesz)?)?;
        let desc_end = name_end.checked_add(descsz)?;
        let name = b.get(12..12 + namesz)?;
        let desc = b.get(name_end..desc_end)?;
        if ty == TYPE && name == NAME {
            // QPluginMetaData::Header: version, Qt major, Qt minor, arch requirements.
            return (desc.len() > 4 && desc[0] == 1).then(|| desc[4..].to_vec());
        }
        b = b.get(name_end.checked_add(pad(descsz)?)?..)?;
    }
    None
}

#[derive(Clone, Copy)]
struct Endian(bool);

impl Endian {
    fn u16(self, b: &[u8]) -> u16 {
        let a = [b[0], b[1]];
        if self.0 {
            u16::from_be_bytes(a)
        } else {
            u16::from_le_bytes(a)
        }
    }
    fn u32(self, b: &[u8]) -> u32 {
        let a = [b[0], b[1], b[2], b[3]];
        if self.0 {
            u32::from_be_bytes(a)
        } else {
            u32::from_le_bytes(a)
        }
    }
    fn u64(self, b: &[u8]) -> u64 {
        let a: [u8; 8] = b[..8].try_into().unwrap_or_default();
        if self.0 {
            u64::from_be_bytes(a)
        } else {
            u64::from_le_bytes(a)
        }
    }
}

/// A decoded CBOR item (RFC 8949), only as much as plugin metadata needs.
#[derive(Debug, PartialEq)]
enum Cbor {
    Int,
    Text(String),
    Array(Vec<Cbor>),
    Map(Vec<(Cbor, Cbor)>),
    Uint(u64),
    Other,
}

struct CborReader<'a>(&'a [u8]);

impl CborReader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let (&b, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(b)
    }

    fn take(&mut self, n: u64) -> Option<&[u8]> {
        let n = usize::try_from(n).ok()?;
        (n <= self.0.len()).then(|| {
            let (a, b) = self.0.split_at(n);
            self.0 = b;
            a
        })
    }

    /// The argument of an initial byte's additional information; None = indefinite length.
    fn arg(&mut self, info: u8) -> Option<Option<u64>> {
        Some(Some(match info {
            0..24 => u64::from(info),
            24 => u64::from(self.byte()?),
            25 => u64::from(u16::from_be_bytes(self.take(2)?.try_into().ok()?)),
            26 => u64::from(u32::from_be_bytes(self.take(4)?.try_into().ok()?)),
            27 => u64::from_be_bytes(self.take(8)?.try_into().ok()?),
            31 => return Some(None),
            _ => return None,
        }))
    }

    fn at_break(&mut self) -> bool {
        let brk = self.0.first() == Some(&0xff);
        if brk {
            self.0 = &self.0[1..];
        }
        brk
    }

    /// Whether a container of `len` items (None: indefinite, up to a break) has more after `done`.
    fn more(&mut self, len: Option<u64>, done: usize) -> bool {
        match len {
            Some(n) => (done as u64) < n,
            None => !self.at_break(),
        }
    }

    fn item(&mut self, depth: u8) -> Option<Cbor> {
        let depth = depth.checked_sub(1)?;
        let head = self.byte()?;
        let (major, arg) = (head >> 5, self.arg(head & 0x1f)?);
        Some(match (major, arg) {
            (0, Some(n)) => Cbor::Uint(n),
            (1, Some(_)) => Cbor::Int,
            (2, Some(n)) => {
                self.take(n)?;
                Cbor::Other
            }
            (3, Some(n)) => Cbor::Text(std::str::from_utf8(self.take(n)?).ok()?.to_string()),
            (3, None) => {
                let mut s = String::new();
                while !self.at_break() {
                    match self.item(depth)? {
                        Cbor::Text(t) => s.push_str(&t),
                        _ => return None,
                    }
                }
                Cbor::Text(s)
            }
            (4, n) => {
                let mut v = vec![];
                while self.more(n, v.len()) {
                    v.push(self.item(depth)?);
                }
                Cbor::Array(v)
            }
            (5, n) => {
                let mut v = vec![];
                while self.more(n, v.len()) {
                    v.push((self.item(depth)?, self.item(depth)?));
                }
                Cbor::Map(v)
            }
            (6, Some(_)) => {
                self.item(depth)?;
                Cbor::Other
            }
            (7, Some(_)) => Cbor::Other,
            _ => return None,
        })
    }
}

/// `MetaData.Keys` of the plugin metadata map (QtPluginMetaDataKeys::MetaData = 4).
fn metadata_keys(cbor: &[u8]) -> Vec<String> {
    let Some(Cbor::Map(top)) = CborReader(cbor).item(16) else {
        return vec![];
    };
    fn field<'a>(m: &'a [(Cbor, Cbor)], k: &Cbor) -> Option<&'a Cbor> {
        m.iter().find(|(a, _)| a == k).map(|(_, v)| v)
    }
    let Some(Cbor::Map(meta)) = field(&top, &Cbor::Uint(4)) else {
        return vec![];
    };
    match field(meta, &Cbor::Text("Keys".into())) {
        Some(Cbor::Array(keys)) => keys
            .iter()
            .filter_map(|k| match k {
                Cbor::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

pub fn platform_theme() -> PlatformTheme {
    static THEME: OnceLock<PlatformTheme> = OnceLock::new();
    *THEME.get_or_init(resolve_platform_theme)
}

fn resolve_platform_theme() -> PlatformTheme {
    resolve_platform_theme_in(&crate::xdg::var, &installed)
}

/// [`resolve_platform_theme`] over the variables `env` gives and the plugins `installed` has.
fn resolve_platform_theme_in(
    env: &crate::xdg::Env,
    installed: &dyn Fn(&str) -> bool,
) -> PlatformTheme {
    use PlatformTheme::*;
    let mut names: Vec<String> = env("QT_QPA_PLATFORMTHEME")
        .unwrap_or_default()
        .split(':')
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .collect();
    let desktop = env("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_uppercase();
    for d in desktop.split(':').filter(|d| !d.is_empty()) {
        if d == "KDE" {
            names.push("kde".into());
        } else if [
            "GNOME",
            "X-CINNAMON",
            "PANTHEON",
            "UNITY",
            "MATE",
            "XFCE",
            "LXDE",
        ]
        .contains(&d)
        {
            names.extend(["gtk3".into(), "gnome".into()]);
        } else {
            let d = d.to_ascii_lowercase();
            names.push(d.strip_prefix("x-").unwrap_or(&d).to_string());
        }
    }
    names.push("generic".into());
    let plugin = |n: &str| match n {
        "kde" => installed(n).then_some(KdePlugin),
        "gtk3" => installed(n).then_some(Gtk3),
        "qt6ct" | "qt5ct" => installed(n).then_some(Qt6ct),
        "xdgdesktopportal" | "flatpak" | "snap" => installed(n).then_some(XdgPortal),
        _ => None,
    };
    let builtin = |n: &str| match n {
        "generic" => Some(Generic),
        "kde" => env("KDE_SESSION_VERSION")
            .and_then(|v| v.parse::<i32>().ok())
            .is_some_and(|v| v >= 4)
            .then_some(Kde),
        "gnome" => Some(Gnome),
        _ => None,
    };
    names
        .iter()
        .find_map(|n| plugin(n))
        .or_else(|| names.iter().find_map(|n| builtin(n)))
        .unwrap_or(Generic)
}

pub fn config_dir() -> String {
    crate::xdg::s(crate::xdg::config_home())
}

fn config_text(file: &str) -> Option<String> {
    std::fs::read_to_string(Path::new(&config_dir()).join(file)).ok()
}

/// QPlatformTheme::SystemFont: the application font whose properties every QML `font` that
/// leaves them unset inherits (QFont resolves against QGuiApplication::font()).
#[derive(Clone, Debug, PartialEq)]
pub struct SystemFont {
    pub family: String,
    pub point_size: f64,
    pub pixel_size: i32,
    pub weight: i32,
    pub style_name: String,
}

impl SystemFont {
    /// QGenericUnixTheme: "Sans Serif" 9pt, which fontconfig resolves like `sans-serif`.
    fn generic() -> Self {
        SystemFont {
            family: "Sans Serif".into(),
            point_size: 9.0,
            pixel_size: -1,
            weight: 400,
            style_name: String::new(),
        }
    }

    /// What QFont::family() gives: the generic theme's font is held here under the name
    /// fontconfig knows it by.
    pub fn qt_family(&self) -> &str {
        match self.family.as_str() {
            "sans-serif" => "Sans Serif",
            f => f,
        }
    }

    /// Pixel size at Qt's 96 dpi.
    pub fn pixels(&self) -> f64 {
        if self.pixel_size > 0 {
            self.pixel_size as f64
        } else {
            self.point_size * 96.0 / 72.0
        }
    }
}

pub fn system_font() -> Arc<SystemFont> {
    static FONT: Live<Arc<SystemFont>> = Live::new();
    FONT.get(|| {
        let font = config_text("kdeglobals").and_then(|t| crate::ini::value(&t, "General", "font"));
        Arc::new(system_font_of(platform_theme(), font.as_deref()))
    })
}

/// [`system_font`] of `theme` with kdeglobals' `[General] font` being `font`.
fn system_font_of(theme: PlatformTheme, font: Option<&str>) -> SystemFont {
    match theme {
        // QKdeThemePrivate::kdeFont: the description over QFont(family), whose size is
        // QFont's own 12 points and not the theme's 9; the generic font where there is no
        // description or QFont::fromString refuses it.
        PlatformTheme::Kde => font
            .and_then(|v| {
                parse_qfont(
                    v,
                    SystemFont {
                        point_size: 12.0,
                        ..SystemFont::generic()
                    },
                )
            })
            .unwrap_or_else(SystemFont::generic),
        // plasma-integration's KFontSettingsData: the description over its default font.
        PlatformTheme::KdePlugin => {
            let default = SystemFont {
                family: "Noto Sans".into(),
                point_size: 10.0,
                ..SystemFont::generic()
            };
            font.and_then(|v| parse_qfont(v, default.clone()))
                .unwrap_or(default)
        }
        _ => SystemFont::generic(),
    }
}

/// QFont::fromString over `base` (the QFont(family) the description is applied to).
fn parse_qfont(desc: &str, base: SystemFont) -> Option<SystemFont> {
    let l: Vec<&str> = desc.trim().split(',').collect();
    let n = l.len();
    if (n > 2 && n < 10) || n > 17 || l[0].is_empty() {
        return None;
    }
    let mut f = SystemFont {
        family: l[0].to_string(),
        ..base
    };
    if n > 1
        && let Ok(pt) = l[1].trim().parse::<f64>()
        && pt > 0.0
    {
        f.point_size = pt;
        f.pixel_size = -1;
    }
    if n >= 10 {
        let int = |k: usize| l[k].trim().parse::<i32>().unwrap_or(0);
        if int(2) > 0 {
            f.pixel_size = int(2);
        }
        f.weight = if n >= 16 {
            int(4)
        } else {
            legacy_weight(int(4))
        };
        f.style_name = if n == 11 || n == 17 {
            l[n - 1].to_string()
        } else {
            String::new()
        };
    }
    Some(f)
}

/// qt_legacyToOpenTypeWeight: the Qt 5 weight scale (0-99) to OpenType, nearest entry.
fn legacy_weight(w: i32) -> i32 {
    const MAP: [(i32, i32); 9] = [
        (0, 100),
        (12, 200),
        (25, 300),
        (50, 400),
        (57, 500),
        (63, 600),
        (75, 700),
        (81, 800),
        (87, 900),
    ];
    let w = w.clamp(0, 99);
    MAP.iter()
        .min_by_key(|(l, _)| (l - w).abs())
        .map_or(400, |m| m.1)
}

/// Qt::ColorScheme of the platform theme (QStyleHints::colorScheme).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorScheme {
    /// The theme reports none.
    Unknown = 0,
    Light = 1,
    Dark = 2,
}

pub fn color_scheme() -> ColorScheme {
    use ColorScheme::*;
    static SCHEME: Live<ColorScheme> = Live::new();
    let of = |dark: bool| if dark { Dark } else { Light };
    SCHEME.get(|| match platform_theme() {
        PlatformTheme::KdePlugin => of(kde_plugin_dark()),
        PlatformTheme::Kde => kde_builtin_scheme(&config_text("kdeglobals").unwrap_or_default()),
        PlatformTheme::Gnome | PlatformTheme::XdgPortal => {
            watch();
            of(*PORTAL_SCHEME.lock().unwrap_or_else(|e| e.into_inner()) == Some(1))
        }
        PlatformTheme::Gtk3 => of(gtk3_dark()),
        PlatformTheme::Qt6ct | PlatformTheme::Generic => Unknown,
    })
}

pub fn system_dark() -> bool {
    color_scheme() == ColorScheme::Dark
}

/// QStyleHints::cursorFlashTime in ms: the platform theme's QPlatformTheme::CursorFlashTime.
/// A text caret is on for half of it and off for the other half; under 2 it never blinks
/// (QQuickTextInputPrivate::updateCursorBlinking starts no timer and the caret stays on).
pub fn cursor_flash_time() -> i32 {
    static TIME: Live<i32> = Live::new();
    TIME.get(|| cursor_flash_time_of(platform_theme(), &config_text))
}

/// [`cursor_flash_time`] of `theme` over the files `config` gives from the config directory.
fn cursor_flash_time_of(theme: PlatformTheme, config: &dyn Fn(&str) -> Option<String>) -> i32 {
    /// QPlatformTheme::defaultThemeHint.
    const DEFAULT: i32 = 1000;
    let setting = |file: &str, section: &str, key: &str| {
        config(file).and_then(|t| crate::ini::value(&t, section, key))
    };
    // QVariant::toInt of a settings string: 0 unless it is an integer ("700.6" is 0 too).
    let int = |v: String| v.trim().parse::<i32>().unwrap_or(0);
    // kdeglobals [KDE] CursorBlinkRate, kept within 200..2000, and 0 ("no blinking") when not
    // positive.
    let kde = |rate: i32| {
        if rate > 0 { rate.clamp(200, 2000) } else { 0 }
    };
    let blink_rate = || setting("kdeglobals", "KDE", "CursorBlinkRate");
    match theme {
        // QKdeThemePrivate::refresh: the key's QVariant::toInt where the key is there.
        PlatformTheme::Kde => blink_rate().map_or(DEFAULT, |v| kde(int(v))),
        // plasma-integration's KHintsSettings: KConfigGroup::readEntry with an int default,
        // which is that default (1000) for a value that is no integer.
        PlatformTheme::KdePlugin => kde(blink_rate()
            .and_then(|v| v.trim().parse::<i32>().ok())
            .unwrap_or(DEFAULT)),
        // Qt6CTPlatformTheme::readSettings.
        PlatformTheme::Qt6ct => {
            setting("qt6ct/qt6ct.conf", "Interface", "cursor_flash_time").map_or(DEFAULT, int)
        }
        // QGtk3Theme::themeHint: GtkSettings' gtk-cursor-blink (on) and gtk-cursor-blink-time
        // (1200), read where `gtk3_dark` reads the theme.
        PlatformTheme::Gtk3 => {
            let gtk = |key: &str| setting("gtk-3.0/settings.ini", "Settings", key);
            match gtk("gtk-cursor-blink").as_deref() {
                Some("0" | "false") => 0,
                _ => gtk("gtk-cursor-blink-time").map_or(1200, int),
            }
        }
        PlatformTheme::Gnome | PlatformTheme::XdgPortal | PlatformTheme::Generic => DEFAULT,
    }
}

/// KConfigGroup::readEntry of a QColor (KConfigGroupGui's readEntryGui) with `default` for a
/// missing key: `r,g,b` or `r,g,b,a` of 0..255 each, else `default`; a value with no comma or
/// starting with `#` is what QColor::fromString makes of it; an empty one, `invalid` and a
/// name QColor does not know are QColor(), which paints black.
fn kconfig_color(value: Option<&str>, default: [u8; 4]) -> [u8; 4] {
    let Some(v) = value else { return default };
    if v.is_empty() || v == "invalid" {
        return [0, 0, 0, 255];
    }
    if v.starts_with('#') || !v.contains(',') {
        let c = crate::builtins::parse_color(v);
        return match c.spec {
            crate::color::Spec::Invalid => [0, 0, 0, 255],
            _ => c.rgba8(),
        };
    }
    let fields: Vec<_> = v.split(',').map(|f| f.trim().parse::<u8>().ok()).collect();
    match fields[..] {
        [Some(r), Some(g), Some(b)] => [r, g, b, 255],
        [Some(r), Some(g), Some(b), Some(a)] => [r, g, b, a],
        _ => default,
    }
}

/// A colour of the scheme plasma-integration builds its palette from, `default` (KColorScheme's
/// own, Breeze light) where the scheme has no such key.
fn kde_plugin_color(scheme: Option<&str>, set: &str, key: &str, default: [u8; 3]) -> [u8; 4] {
    let [r, g, b] = default;
    let value = scheme.and_then(|s| crate::ini::value(s, set, key));
    kconfig_color(value.as_deref(), [r, g, b, 255])
}

fn data_file(rel: &str) -> Option<String> {
    let data_home = crate::xdg::s(crate::xdg::data_home());
    let dirs = crate::icons::xdg_data_dirs();
    std::iter::once(data_home.as_str())
        .chain(dirs.iter().map(String::as_str))
        .find_map(|d| std::fs::read_to_string(Path::new(d).join(rel)).ok())
}

/// KHintsSettings::loadPalettes + determineColorScheme: the KColorScheme window background of
/// kdeglobals (when it has `[Colors:View]`), else of the look-and-feel package or the
/// `General/ColorScheme` scheme file (BreezeLight); qGray below 192 is Dark.
fn kde_plugin_dark() -> bool {
    let [r, g, b, _] = kde_plugin_color(
        kde_plugin_scheme().as_deref(),
        "Colors:Window",
        "BackgroundNormal",
        [239, 240, 241],
    )
    .map(i32::from);
    (r * 11 + g * 16 + b * 5) / 32 < 192
}

/// The color scheme plasma-integration builds its palette from (see `kde_plugin_dark`).
fn kde_plugin_scheme() -> Option<String> {
    let globals = config_text("kdeglobals").unwrap_or_default();
    if globals.lines().any(|l| l.trim() == "[Colors:View]") {
        Some(globals.clone())
    } else {
        let lnf = crate::ini::value(&globals, "KDE", "LookAndFeelPackage")
            .unwrap_or_else(|| "org.kde.breeze.desktop".into());
        data_file(&format!("plasma/look-and-feel/{lnf}/contents/colors")).or_else(|| {
            let name = crate::ini::value(&globals, "General", "ColorScheme")
                .unwrap_or_else(|| "BreezeLight".into());
            data_file(&format!("color-schemes/{name}.colors"))
                .or_else(|| data_file("color-schemes/BreezeLight.colors"))
        })
    }
}

/// KIconColors(qApp->palette()).stylesheet(Normal): what plasma-integration's KIconEngine puts
/// into a color-scheme-following SVG icon. None without the plugin (no recoloring).
pub fn kde_icon_stylesheet() -> Option<Arc<str>> {
    static CSS: Live<Option<Arc<str>>> = Live::new();
    CSS.get(|| {
        (platform_theme() == PlatformTheme::KdePlugin).then(|| {
            let s = kde_plugin_scheme();
            let c = |set: &str, key: &str, d: [u8; 3]| {
                let [r, g, b, _] = kde_plugin_color(s.as_deref(), set, key, d);
                format!("#{r:02x}{g:02x}{b:02x}")
            };
            let text = c("Colors:Window", "ForegroundNormal", [35, 38, 41]);
            let bg = c("Colors:Window", "BackgroundNormal", [239, 240, 241]);
            let hl = c("Colors:Selection", "BackgroundNormal", [61, 174, 233]);
            let hl_text = c("Colors:Selection", "ForegroundNormal", [255, 255, 255]);
            let pos = c("Colors:Window", "ForegroundPositive", [39, 174, 96]);
            let neu = c("Colors:Window", "ForegroundNeutral", [246, 116, 0]);
            let neg = c("Colors:Window", "ForegroundNegative", [218, 68, 83]);
            format!(
                ".ColorScheme-Text {{ color:{text}; }}.ColorScheme-Background{{ color:{bg}; }}\
                 .ColorScheme-Highlight{{ color:{hl}; }}.ColorScheme-HighlightedText{{ color:{hl_text}; }}\
                 .ColorScheme-PositiveText{{ color:{pos}; }}.ColorScheme-NeutralText{{ color:{neu}; }}\
                 .ColorScheme-NegativeText{{ color:{neg}; }}.ColorScheme-Accent{{ color:{hl}; }}"
            )
            .into()
        })
    })
}

/// QPalette::Link of the application palette as `[r, g, b, a]`: the View set's ForegroundLink
/// under KDE (KColorScheme, QKdeTheme), else Qt's default palette link (Qt::blue).
pub fn link_color() -> [u8; 4] {
    static LINK: Live<[u8; 4]> = Live::new();
    LINK.get(|| match platform_theme() {
        PlatformTheme::KdePlugin => kde_plugin_color(
            kde_plugin_scheme().as_deref(),
            "Colors:View",
            "ForegroundLink",
            [41, 128, 185],
        ),
        PlatformTheme::Kde => {
            kde_builtin_palette(&config_text("kdeglobals").unwrap_or_default()).link
        }
        _ => [0, 0, 255, 255],
    })
}

/// The roles of QKdeTheme's system palette that the shell reads, as `[r, g, b, a]`.
struct KdePalette {
    text: [u8; 4],
    base: [u8; 4],
    link: [u8; 4],
}

/// QKdeThemePrivate::readKdeSystemPalette over the kdeglobals `globals`. A colour is a
/// QSettings list of exactly three fields, each its QVariant::toInt (0 for what is no
/// integer); QColor of a field outside 0..255 is invalid, which reads as transparent black.
/// Without such a list in `[Colors:Button] BackgroundNormal` the file's colours are not read
/// at all and the palette is QPalette(button, window) of KDE's defaults; with it a role the
/// file leaves out stays as QPalette() has it.
fn kde_builtin_palette(globals: &str) -> KdePalette {
    let color = |set: &str, key: &str| {
        let v = crate::ini::value(globals, set, key)?;
        let fields: Vec<_> = v
            .split(',')
            .map(|f| f.trim().parse::<i32>().unwrap_or(0))
            .collect();
        let [r, g, b] = fields[..] else { return None };
        let byte = |c: i32| u8::try_from(c).ok();
        Some(match (byte(r), byte(g), byte(b)) {
            (Some(r), Some(g), Some(b)) => [r, g, b, 255],
            _ => [0; 4],
        })
    };
    const BLACK: [u8; 4] = [0, 0, 0, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    if color("Colors:Button", "BackgroundNormal").is_none() {
        return KdePalette {
            text: BLACK,
            base: WHITE,
            link: BLUE,
        };
    }
    KdePalette {
        text: color("Colors:View", "ForegroundNormal").unwrap_or(WHITE),
        base: color("Colors:View", "BackgroundNormal").unwrap_or(BLACK),
        link: color("Colors:View", "ForegroundLink").unwrap_or(BLUE),
    }
}

/// QKdeThemePrivate::updateColorScheme: "light"/"dark" in `General/ColorScheme`, else
/// QColor::lightness of the system palette's text against its base, Unknown where they are
/// the same.
fn kde_builtin_scheme(globals: &str) -> ColorScheme {
    let name = crate::ini::value(globals, "General", "ColorScheme")
        .unwrap_or_default()
        .to_lowercase();
    if name.contains("light") {
        return ColorScheme::Light;
    }
    if name.contains("dark") {
        return ColorScheme::Dark;
    }
    // QColor::lightness: the HSL lightness of the 16-bit colour, shifted down to 8 bits.
    let lightness = |[r, g, b, a]: [u8; 4]| {
        let argb = u32::from_be_bytes([a, r, g, b]);
        crate::color::Color::from_argb32(argb).to_hsl().c[2] >> 8
    };
    let palette = kde_builtin_palette(globals);
    match lightness(palette.text).cmp(&lightness(palette.base)) {
        std::cmp::Ordering::Less => ColorScheme::Light,
        std::cmp::Ordering::Greater => ColorScheme::Dark,
        std::cmp::Ordering::Equal => ColorScheme::Unknown,
    }
}

/// QGtk3Theme: the GTK theme's own scheme (dark variant requested or a "dark" theme name).
fn gtk3_dark() -> bool {
    let s = config_text("gtk-3.0/settings.ini").unwrap_or_default();
    let v = |k: &str| crate::ini::value(&s, "Settings", k).unwrap_or_default();
    matches!(
        v("gtk-application-prefer-dark-theme").as_str(),
        "1" | "true"
    ) || v("gtk-theme-name").to_ascii_lowercase().contains("dark")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kdeglobals_font_description() {
        let f = parse_qfont(
            "Google Sans Flex,11,-1,5,500,0,0,0,0,0,0,0,0,0,0,1,Medium",
            SystemFont::generic(),
        )
        .unwrap();
        assert_eq!(
            (
                f.family.as_str(),
                f.point_size,
                f.weight,
                f.style_name.as_str()
            ),
            ("Google Sans Flex", 11.0, 500, "Medium")
        );
        assert!((f.pixels() - 14.666).abs() < 0.01);
        let f = parse_qfont("Noto Sans,10,-1,5,50,0,0,0,0,0", SystemFont::generic()).unwrap();
        assert_eq!((f.weight, f.style_name.as_str()), (400, ""));
        assert_eq!(parse_qfont("Noto Sans,10,-1", SystemFont::generic()), None);
    }

    /// Qt 6 plugin metadata: indefinite map {2: IID, 3: class name, 4: {"Keys": [...]}}.
    fn metadata(class: &str, keys: &[&str]) -> Vec<u8> {
        let text = |b: &mut Vec<u8>, t: &str| {
            assert!(t.len() < 256);
            b.extend([0x78, t.len() as u8]);
            b.extend(t.as_bytes());
        };
        let mut b = vec![0xbf, 0x02];
        text(
            &mut b,
            "org.qt-project.QPA.QPlatformThemeFactoryInterface.5.1",
        );
        b.push(0x03);
        text(&mut b, class);
        b.extend([0x04, 0xa1, 0x64]);
        b.extend(b"Keys");
        b.push(0x80 | keys.len() as u8);
        for k in keys {
            text(&mut b, k);
        }
        b.push(0xff);
        b
    }

    /// A minimal ELF shared object whose `.note.qt.metadata` holds `cbor`, the way moc's
    /// Q_PLUGIN_METADATA emits it; section header table at the end of the file.
    fn elf(wide: bool, be: bool, note_section: &str, cbor: &[u8]) -> Vec<u8> {
        let e = Endian(be);
        let u16b = |v: u16| {
            if e.0 {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let u32b = |v: u32| {
            if e.0 {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let word = |b: &mut Vec<u8>, v: u64| {
            if wide {
                b.extend(if e.0 {
                    v.to_be_bytes()
                } else {
                    v.to_le_bytes()
                });
            } else {
                b.extend(u32b(v as u32));
            }
        };
        let ehsize = if wide { 64 } else { 52 };
        let mut note = vec![];
        // An unrelated note first: the parser walks the records.
        note.extend(u32b(4));
        note.extend(u32b(4));
        note.extend(u32b(3));
        note.extend(b"GNU\0\0\0\0\0");
        let mut desc = vec![1, 6, 8, 0];
        desc.extend(cbor);
        note.extend(u32b(12));
        note.extend(u32b(desc.len() as u32));
        note.extend(u32b(0x7451_0001));
        note.extend(b"qt-project!\0");
        note.extend(&desc);
        while note.len() % 4 != 0 {
            note.push(0);
        }
        let names = format!("\0{note_section}\0.shstrtab\0").into_bytes();
        let note_off = ehsize as u64;
        let names_off = note_off + note.len() as u64;
        let shoff = names_off + names.len() as u64;
        let mut b = vec![
            0x7f,
            b'E',
            b'L',
            b'F',
            if wide { 2 } else { 1 },
            if be { 2 } else { 1 },
            1,
        ];
        b.resize(16, 0);
        b.extend(u16b(3));
        b.extend(u16b(62));
        b.extend(u32b(1));
        word(&mut b, 0);
        word(&mut b, 0);
        word(&mut b, shoff);
        b.extend(u32b(0));
        b.extend(u16b(ehsize));
        b.extend(u16b(0));
        b.extend(u16b(0));
        b.extend(u16b(if wide { 64 } else { 40 }));
        b.extend(u16b(3));
        b.extend(u16b(2));
        assert_eq!(b.len(), ehsize as usize);
        b.extend(&note);
        b.extend(&names);
        let shdr = |b: &mut Vec<u8>, name: u32, ty: u32, off: u64, size: u64| {
            b.extend(u32b(name));
            b.extend(u32b(ty));
            word(b, 0);
            word(b, 0);
            word(b, off);
            word(b, size);
            b.extend(u32b(0));
            b.extend(u32b(0));
            word(b, 4);
            word(b, 0);
        };
        shdr(&mut b, 0, 0, 0, 0);
        shdr(&mut b, 1, 7, note_off, note.len() as u64);
        shdr(
            &mut b,
            note_section.len() as u32 + 2,
            3,
            names_off,
            names.len() as u64,
        );
        b
    }

    #[test]
    fn plugin_metadata_keys() {
        let kde = metadata("KdePlatformThemePlugin", &["kde"]);
        assert_eq!(metadata_keys(&kde), ["kde"]);
        let portal = metadata(
            "QXdgDesktopPortalThemePlugin",
            &["xdgdesktopportal", "flatpak", "snap"],
        );
        assert_eq!(
            metadata_keys(&portal),
            ["xdgdesktopportal", "flatpak", "snap"]
        );
        // The old byte search's tag outside the MetaData field is not a key list.
        let mut fake = vec![0xa2, 0x03, 0x68];
        fake.extend(b"\xa1\x64Keys");
        fake.extend([0x04, 0xa0]);
        assert!(metadata_keys(&fake).is_empty());
        assert!(metadata_keys(b"\x7fELF no metadata").is_empty());
        assert!(metadata_keys(&kde[..kde.len() - 3]).is_empty());
    }

    #[test]
    fn plugin_metadata_from_elf_note() {
        let cbor = metadata("QGtk3ThemePlugin", &["gtk3"]);
        for (wide, be) in [(true, false), (false, false), (true, true), (false, true)] {
            let lib = elf(wide, be, ".note.qt.metadata", &cbor);
            let got = qt_metadata(&mut std::io::Cursor::new(lib));
            assert_eq!(got.as_deref(), Some(&cbor[..]), "wide={wide} be={be}");
        }
        // The same bytes in another section (e.g. string data) are not plugin metadata.
        let other = elf(true, false, ".note.other", &cbor);
        assert_eq!(qt_metadata(&mut std::io::Cursor::new(other)), None);
        let mut cut = elf(true, false, ".note.qt.metadata", &cbor);
        cut.truncate(cut.len() - 10);
        assert_eq!(qt_metadata(&mut std::io::Cursor::new(cut)), None);
        assert_eq!(
            qt_metadata(&mut std::io::Cursor::new(b"not an elf".to_vec())),
            None
        );
    }

    #[test]
    fn installed_plugins_metadata() {
        // Every Qt 6 platform theme plugin on this machine yields its keys from its note alone.
        let Ok(rd) = std::fs::read_dir("/usr/lib/qt6/plugins/platformthemes") else {
            crate::testenv::unavailable("no Qt 6 platform theme plugins (qt6-base)");
            return;
        };
        for e in rd
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".so"))
        {
            let mut f = std::fs::File::open(e.path()).unwrap();
            let keys = qt_metadata(&mut f)
                .map(|m| metadata_keys(&m))
                .unwrap_or_default();
            assert!(!keys.is_empty(), "{:?}", e.path());
        }
    }

    #[test]
    fn portal_scheme_values() {
        use zbus::zvariant::Value;
        assert_eq!(scheme_value(&Value::U32(1)), Some(1));
        // Settings v1 `Read`: the setting's variant inside the reply's variant.
        let v1 = Value::Value(Box::new(Value::Value(Box::new(Value::U32(2)))));
        assert_eq!(scheme_value(&v1), Some(2));
        assert_eq!(scheme_value(&Value::Str("dark".into())), None);
    }

    /// A fake xdg-desktop-portal on a private bus: `v1` implements only Settings version 1's
    /// `Read`, otherwise `ReadOne`; `missing` answers NotFound.
    fn fake_portal(v1: bool, missing: bool) -> Option<(Daemon, zbus::blocking::Connection)> {
        use std::io::BufRead as _;
        use std::process::{Command, Stdio};
        use zbus::zvariant::Value;
        let Ok(mut child) = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            crate::testenv::unavailable("no dbus-daemon");
            return None;
        };
        let mut addr = String::new();
        std::io::BufReader::new(child.stdout.take()?)
            .read_line(&mut addr)
            .ok()?;
        let daemon = Daemon(child);
        let addr = addr.trim().to_string();
        let portal = zbus::blocking::connection::Builder::address(addr.as_str())
            .ok()?
            .name(PORTAL)
            .ok()?
            .build()
            .ok()?;
        let calls = zbus::blocking::MessageIterator::from(&portal);
        std::thread::spawn(move || {
            for m in calls.flatten() {
                if m.message_type() != zbus::message::Type::MethodCall {
                    continue;
                }
                let h = m.header();
                let member = h.member().map(|n| n.to_string()).unwrap_or_default();
                let err = |name: &str| zbus::Message::error(&h, name).and_then(|b| b.build(&("",)));
                let r = match (member.as_str(), v1, missing) {
                    (_, _, true) => err("org.freedesktop.portal.Error.NotFound"),
                    ("ReadOne", false, _) => {
                        zbus::Message::method_return(&h).and_then(|b| b.build(&Value::U32(1)))
                    }
                    ("Read", true, _) => zbus::Message::method_return(&h)
                        .and_then(|b| b.build(&Value::Value(Box::new(Value::U32(1))))),
                    _ => err("org.freedesktop.DBus.Error.UnknownMethod"),
                };
                if let Ok(r) = r {
                    let _ = portal.send(&r);
                }
            }
        });
        let client = zbus::blocking::connection::Builder::address(addr.as_str())
            .ok()?
            .build()
            .ok()?;
        Some((daemon, client))
    }

    struct Daemon(std::process::Child);

    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn portal_read_one_then_v1_read() {
        for (v1, missing, want) in [(false, false, 1), (true, false, 1), (false, true, 0)] {
            let Some((_daemon, c)) = fake_portal(v1, missing) else {
                return;
            };
            assert_eq!(
                read_portal_scheme(&c).unwrap(),
                want,
                "v1={v1} missing={missing}"
            );
        }
    }

    #[test]
    fn setting_changed_updates_scheme_and_requests_poll() {
        use zbus::zvariant::Value;
        let before = SEEN.load(Ordering::Acquire);
        let m = zbus::Message::signal(PORTAL_PATH, SETTINGS, "SettingChanged")
            .unwrap()
            .build(&(APPEARANCE, "color-scheme", Value::U32(1)))
            .unwrap();
        on_signal(&m);
        assert_eq!(*PORTAL_SCHEME.lock().unwrap(), Some(1));
        assert!(SEEN.load(Ordering::Acquire) > before);
        let other = zbus::Message::signal(PORTAL_PATH, SETTINGS, "SettingChanged")
            .unwrap()
            .build(&("org.freedesktop.a11y", "contrast", Value::U32(0)))
            .unwrap();
        let seen = SEEN.load(Ordering::Acquire);
        on_signal(&other);
        assert_eq!(SEEN.load(Ordering::Acquire), seen);
        let kconfig =
            zbus::Message::signal("/kdeglobals", "org.kde.kconfig.notify", "ConfigChanged")
                .unwrap()
                .build(&std::collections::HashMap::<String, Vec<Vec<u8>>>::new())
                .unwrap();
        on_signal(&kconfig);
        assert!(SEEN.load(Ordering::Acquire) > seen);
        // The runtime applies it once: a new revision re-evaluates and repaints everything.
        let mut w = crate::runtime::World::default();
        let rev = w.revision;
        assert!(poll(&mut w));
        assert_eq!(generation(), SEEN.load(Ordering::Acquire));
        assert!(w.revision > rev);
        assert!(!poll(&mut w));
    }

    #[test]
    fn live_values_follow_applied_changes() {
        static N: Live<u32> = Live::new();
        let calls = std::cell::Cell::new(0);
        let get = || {
            N.get(|| {
                calls.set(calls.get() + 1);
                calls.get()
            })
        };
        let first = get();
        assert_eq!(get(), first);
        APPLIED.fetch_add(1, Ordering::AcqRel);
        assert_eq!(get(), first + 1);
        assert_eq!(get(), first + 1);
    }

    #[test]
    fn hyprland_without_kde_plugin_is_generic() {
        let env = |kde: Option<&'static str>| {
            move |k: &str| match k {
                "QT_QPA_PLATFORMTHEME" => Some("kde".to_string()),
                "XDG_CURRENT_DESKTOP" => Some("Hyprland".to_string()),
                "KDE_SESSION_VERSION" => kde.map(str::to_string),
                _ => None,
            }
        };
        let none = |_: &str| false;
        assert_eq!(
            resolve_platform_theme_in(&env(None), &none),
            PlatformTheme::Generic
        );
        assert_eq!(
            resolve_platform_theme_in(&env(Some("6")), &none),
            PlatformTheme::Kde
        );
        assert_eq!(
            resolve_platform_theme_in(&env(None), &|k| k == "kde"),
            PlatformTheme::KdePlugin
        );
    }
}
