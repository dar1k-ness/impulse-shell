//! freedesktop icon theme lookup behind `Rustshell.iconPath` and `image://icon/` (cached).
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum Kind {
    Fixed,
    Scalable,
    Threshold,
}

struct Dir {
    path: PathBuf,
    size: i32,
    scale: i32,
    kind: Kind,
    min: i32,
    max: i32,
    threshold: i32,
    /// The section of `index.theme`: the directory below the theme's.
    sub: String,
    /// Which of the theme's directories ([`Theme::caches`]) it is in.
    root: usize,
    /// Index in the theme's `Directories` + `ScaledDirectories` list, which KIconTheme reads;
    /// `None` for a section they do not name.
    idx: Option<usize>,
    /// Context Applications or MimeTypes: skipped while dash-truncated names are searched.
    app_or_mime: bool,
    /// Directory listing, read on first lookup (avoids a stat per extension per directory).
    files: std::cell::OnceCell<Listing>,
}

/// The names of a directory's entries, sorted, end to end in one allocation: a name that no
/// theme has makes every directory of every theme list itself (breeze alone is 20k names),
/// and the listings stay for the life of the process.
#[derive(Default)]
struct Listing {
    bytes: Box<[u8]>,
    /// Where each name ends in `bytes`.
    ends: Box<[u32]>,
}

impl Listing {
    fn of(mut names: Vec<std::ffi::OsString>) -> Self {
        names.sort_unstable();
        names.dedup();
        let mut bytes = Vec::with_capacity(names.iter().map(|n| n.len()).sum());
        let mut ends = Vec::with_capacity(names.len());
        for n in &names {
            bytes.extend_from_slice(n.as_encoded_bytes());
            let Ok(end) = u32::try_from(bytes.len()) else {
                bytes.truncate(bytes.len() - n.len());
                break;
            };
            ends.push(end);
        }
        Listing {
            bytes: bytes.into(),
            ends: ends.into(),
        }
    }

    fn name(&self, k: usize) -> &[u8] {
        let from = if k == 0 { 0 } else { self.ends[k - 1] as usize };
        &self.bytes[from..self.ends[k] as usize]
    }

    fn contains(&self, name: &str) -> bool {
        let (mut lo, mut hi) = (0, self.ends.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.name(mid).cmp(name.as_bytes()) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return true,
            }
        }
        false
    }
}

impl Dir {
    fn has(&self, file: &str) -> bool {
        self.files
            .get_or_init(|| {
                std::fs::read_dir(&self.path)
                    .map(|rd| {
                        Listing::of(rd.filter_map(|e| e.ok()).map(|e| e.file_name()).collect())
                    })
                    .unwrap_or_default()
            })
            .contains(file)
    }
}

struct Theme {
    /// QIconTheme::contentDirs outer, QIconTheme::keyList inner.
    dirs: Vec<Dir>,
    /// QIconTheme::m_gtkCaches: the `icon-theme.cache` of each directory of the theme, where
    /// Qt reads it.
    caches: Vec<Option<GtkCache>>,
    inherits: Vec<String>,
}

/// QIconCacheGtkReader: the `icon-theme.cache` gtk-update-icon-cache writes into a theme's
/// directory. It names the directories each icon is in, in its own order.
struct GtkCache {
    data: Vec<u8>,
    /// Cleared by an offset that points outside the file.
    valid: std::cell::Cell<bool>,
}

impl GtkCache {
    /// The cache of `dir` when Qt takes it: of version 1 and not older than `dir` or any
    /// directory it lists. QFileInfo's times are in milliseconds.
    fn read(dir: &Path) -> Option<GtkCache> {
        let ms = |p: &Path| {
            let t = std::fs::metadata(p).ok()?.modified().ok()?;
            Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis())
        };
        let file = dir.join("icon-theme.cache");
        let written = ms(&file)?;
        if ms(dir).is_some_and(|t| written < t) {
            return None;
        }
        let c = GtkCache {
            data: std::fs::read(&file).ok()?,
            valid: std::cell::Cell::new(true),
        };
        if c.u16(0)? != 1 {
            return None;
        }
        let list = c.u32(8)?;
        for i in 0..c.u32(list)? {
            let sub = c.str(c.u32(list + 4 + 4 * i)?)?;
            if ms(&dir.join(&*sub)).is_some_and(|t| written < t) {
                return None;
            }
        }
        Some(c)
    }

    fn u16(&self, at: usize) -> Option<usize> {
        let b = self.data.get(at..at + 2).filter(|_| at.is_multiple_of(2))?;
        Some(u16::from_be_bytes([b[0], b[1]]) as usize)
    }

    fn u32(&self, at: usize) -> Option<usize> {
        let b = self.data.get(at..at + 4).filter(|_| at.is_multiple_of(4))?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }

    /// The string at `at`, which is inside the file.
    fn bytes(&self, at: usize) -> Option<&[u8]> {
        let rest = self.data.get(at..).filter(|r| !r.is_empty())?;
        Some(&rest[..rest.iter().position(|b| *b == 0).unwrap_or(rest.len())])
    }

    fn str(&self, at: usize) -> Option<std::borrow::Cow<'_, str>> {
        Some(String::from_utf8_lossy(self.bytes(at)?))
    }

    /// QIconCacheGtkReader::lookup: the directories that have `name`, in the cache's order.
    /// `None` once the cache is not valid: every directory is looked into then.
    fn lookup(&self, name: &str) -> Option<Vec<std::borrow::Cow<'_, str>>> {
        if !self.valid.get() {
            return None;
        }
        if name.is_empty() {
            return Some(Vec::new());
        }
        let found = self.directories_of(name);
        self.valid.set(found.is_some());
        found
    }

    fn directories_of(&self, name: &str) -> Option<Vec<std::borrow::Cow<'_, str>>> {
        // icon_name_hash: of signed chars.
        let mut hash = name.as_bytes()[0] as i8 as u32;
        for b in &name.as_bytes()[1..] {
            hash = (hash << 5).wrapping_sub(hash).wrapping_add(*b as i8 as u32);
        }
        let table = self.u32(4)?;
        let buckets = self.u32(table)?;
        if buckets == 0 {
            return None;
        }
        let mut at = self.u32(table + 4 + (hash as usize % buckets) * 4)?;
        while at > 0 && at + 12 <= self.data.len() {
            if self.bytes(self.u32(at + 4)?) == Some(name.as_bytes()) {
                let dirs = self.u32(8)?;
                let count = self.u32(dirs)?;
                let list = self.u32(at + 8)?;
                let images = self.u32(list)?;
                if list + 4 + 8 * images > self.data.len() {
                    return None;
                }
                let mut out = Vec::new();
                for j in 0..images {
                    let index = self.u16(list + 4 + 8 * j)?;
                    if index >= count {
                        return None;
                    }
                    out.push(self.str(self.u32(dirs + 4 + index * 4)?)?);
                }
                return Some(out);
            }
            at = self.u32(at)?;
        }
        Some(Vec::new())
    }
}

thread_local! {
    static THEMES: RefCell<HashMap<String, Option<std::rc::Rc<Theme>>>> = Default::default();
    static CACHE: RefCell<HashMap<(String, i32, i32), Option<String>>> = Default::default();
    static FOUND: RefCell<Names> = Default::default();
}

/// lookupFallbackIcon order.
const EXTS: [&str; 3] = ["png", "xpm", "svg"];

fn bases() -> Vec<PathBuf> {
    let mut v = vec![crate::xdg::home().join(".icons")];
    v.push(crate::xdg::data_home().join("icons"));
    v.extend(
        xdg_data_dirs()
            .iter()
            .map(|d| PathBuf::from(d).join("icons")),
    );
    v
}

/// QStandardPaths GenericDataLocation: XDG_DATA_HOME, then XDG_DATA_DIRS.
fn data_dirs() -> Vec<PathBuf> {
    let mut v = vec![crate::xdg::data_home()];
    v.extend(xdg_data_dirs().into_iter().map(PathBuf::from));
    v
}

/// QStandardPaths xdgDataDirs: absolute entries, cleaned, duplicates dropped (a session whose
/// env prepends on every compositor config reload repeats them hundreds of times).
pub(crate) fn xdg_data_dirs() -> Vec<String> {
    static DIRS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    DIRS.get_or_init(|| {
        clean_data_dirs(
            &std::env::var("XDG_DATA_DIRS")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
        )
    })
    .clone()
}

fn clean_data_dirs(dirs: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in dirs.split(':').filter(|d| d.starts_with('/')) {
        let d = Path::new(d)
            .components()
            .collect::<PathBuf>()
            .display()
            .to_string();
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// QIcon::fallbackSearchPaths: Qt's /usr/share/pixmaps, then the `<XDG_DATA_DIRS>/pixmaps`
/// Quickshell's launch appends.
fn fallback_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![];
    let qt = PathBuf::from("/usr/share/pixmaps");
    if qt.is_dir() {
        v.push(qt);
    }
    for d in xdg_data_dirs() {
        let p = PathBuf::from(d.trim_end_matches('/')).join("pixmaps");
        if !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

/// QIconLoader::lookupFallbackIcon: first directory holding `name` as png, xpm or svg.
fn fallback_icon(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().find_map(|d| {
        EXTS.iter()
            .map(|e| d.join(format!("{name}.{e}")))
            .find(|p| p.is_file())
    })
}

/// KIconLoader::iconPath for the User group: `KIconLoaderPrivate::locate` of `name.svg`, then
/// `.png`, `.xpm` in the legacy `icons` and `pixmaps` data dirs (where dots-hyprland installs
/// `~/.local/share/icons/impulse-shell.svg`).
fn user_icon(name: &str, data: &[PathBuf]) -> Option<PathBuf> {
    ["svg", "png", "xpm"].iter().find_map(|e| {
        ["icons", "pixmaps"].iter().find_map(|sub| {
            data.iter()
                .map(|d| d.join(sub).join(format!("{name}.{e}")))
                .find(|p| p.is_file())
        })
    })
}

/// `key` of `[section]` in `<config dir>/<file>`, unquoted, when the file has it.
fn config_entry(file: &str, section: &str, key: &str) -> Option<String> {
    std::fs::read_to_string(Path::new(&crate::colorscheme::config_dir()).join(file))
        .ok()
        .and_then(|t| crate::ini::value(&t, section, key))
        .map(|v| v.trim_matches('"').to_string())
}

/// `key` of `[section]` in `<config dir>/<file>`, unquoted, when set.
fn config_value(file: &str, section: &str, key: &str) -> Option<String> {
    config_entry(file, section, key).filter(|v| !v.is_empty())
}

/// Themes searched once Qt's chain finds nothing (where Quickshell would draw its magenta
/// missing pixmap): the icon theme configured in every toolkit, then the common defaults.
fn last_resort_themes() -> Vec<String> {
    let mut v: Vec<String> = [
        ("kdeglobals", "Icons", "Theme"),
        ("qt6ct/qt6ct.conf", "Appearance", "icon_theme"),
        ("qt5ct/qt5ct.conf", "Appearance", "icon_theme"),
        ("gtk-4.0/settings.ini", "Settings", "gtk-icon-theme-name"),
        ("gtk-3.0/settings.ini", "Settings", "gtk-icon-theme-name"),
    ]
    .iter()
    .filter_map(|(f, s, k)| config_value(f, s, k))
    .collect();
    v.extend(["breeze", "Adwaita", "hicolor"].map(String::from));
    let mut out: Vec<String> = Vec::new();
    for t in v {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// After Qt's chain misses: the exact name in every other installed theme and the unthemed
/// paths, then the name cut at its last dash the same way.
fn last_resort(q: &Query, seen: &mut Vec<String>) -> Option<Found> {
    last_resort_themes()
        .iter()
        .find_map(|t| search(t, q, seen))
        .or_else(|| fallback_icon(q.name, &fallback_dirs()).map(Found::File))
        .or_else(|| user_icon(q.name, &data_dirs()).map(Found::File))
        .or_else(|| {
            let (name, _) = q.name.rsplit_once('-')?;
            let q = Query {
                name,
                generic: true,
                ..*q
            };
            last_resort(&q, &mut Vec::new())
        })
}

/// What a theme icon that resolves nowhere draws instead of a missing-texture checkerboard:
/// the theme's generic application icon, else its missing-image icon.
pub fn generic_icon(size: i32, scale: i32) -> Option<String> {
    lookup_scaled("application-x-executable", size, scale)
        .or_else(|| lookup_scaled("image-missing", size, scale))
}

/// The user's icon theme and the fallback theme searched after it, as the platform theme Qt
/// picked would report them (SystemIconThemeName / SystemIconFallbackThemeName).
pub fn theme_names() -> (String, String) {
    use crate::colorscheme::PlatformTheme::*;
    if let Ok(t) = std::env::var("QS_ICON_THEME")
        && !t.is_empty()
    {
        return (t, "hicolor".into());
    }
    let read = config_value;
    let gtk = || {
        read("gtk-3.0/settings.ini", "Settings", "gtk-icon-theme-name")
            .unwrap_or_else(|| "Adwaita".into())
    };
    match crate::colorscheme::platform_theme() {
        // QKdeTheme: kdeglobals' theme, and its fallback theme (breeze) where that is no name:
        // QIconLoader takes the fallback's name for an empty one.
        Kde => (
            read("kdeglobals", "Icons", "Theme").unwrap_or_else(|| "breeze".into()),
            "breeze".into(),
        ),
        // plasma-integration: KHintsSettings gives the same key with the same default, and
        // hicolor as its fallback theme, whose name QIconLoader takes for an empty one;
        // KIconThemes/BreezeIcons then make breeze the fallback.
        KdePlugin => (
            config_entry("kdeglobals", "Icons", "Theme")
                .map(|t| if t.is_empty() { "hicolor".into() } else { t })
                .unwrap_or_else(|| "breeze".into()),
            "breeze".into(),
        ),
        Gtk3 => (gtk(), "gnome".into()),
        Gnome => ("Adwaita".into(), "gnome".into()),
        Qt6ct => (
            read("qt6ct/qt6ct.conf", "Appearance", "icon_theme")
                .unwrap_or_else(|| "hicolor".into()),
            "hicolor".into(),
        ),
        XdgPortal | Generic => ("hicolor".into(), "hicolor".into()),
    }
}

fn load_theme(name: &str) -> Option<std::rc::Rc<Theme>> {
    if let Some(t) = THEMES.with(|c| c.borrow().get(name).cloned()) {
        return t;
    }
    let roots: Vec<PathBuf> = bases()
        .into_iter()
        .map(|b| b.join(name))
        .filter(|p| p.is_dir())
        .collect();
    let index = roots
        .iter()
        .find_map(|r| std::fs::read_to_string(r.join("index.theme")).ok());
    let theme = index.map(|idx| {
        let ini = crate::ini::sections(&idx);
        let get = |sec: &str, k: &str| ini.get(sec).and_then(|m| m.get(k)).map(String::as_str);
        let list = |k: &str| -> Vec<String> {
            get("Icon Theme", k)
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut listed = list("Directories");
        listed.extend(list("ScaledDirectories"));
        // QIconTheme::QIconTheme: every section with a `Size`, in the order of
        // QSettings::allKeys(), which sorts the keys (`m/16/Size`, `m/16@2x/Size`) by their
        // UTF-16 units.
        let key = |sd: &str| format!("{sd}/Size").encode_utf16().collect::<Vec<u16>>();
        let mut subdirs: Vec<&str> = ini.keys().map(String::as_str).collect();
        subdirs.sort_by_key(|sd| key(sd));
        let mut dirs = Vec::new();
        // QIconTheme::contentDirs outer, keyList inner (QIconLoader::findIconHelper order).
        for (root, r) in roots.iter().enumerate() {
            for sd in &subdirs {
                let num = |k: &str| get(sd, k).and_then(|v| v.parse::<i32>().ok());
                let Some(size) = num("Size").filter(|s| *s != 0) else {
                    continue;
                };
                let kind = match get(sd, "Type") {
                    Some("Fixed") => Kind::Fixed,
                    Some("Scalable") => Kind::Scalable,
                    _ => Kind::Threshold,
                };
                let path = r.join(sd);
                if path.is_dir() {
                    dirs.push(Dir {
                        path,
                        size,
                        scale: num("Scale").unwrap_or(1),
                        kind,
                        min: num("MinSize").unwrap_or(size),
                        max: num("MaxSize").unwrap_or(size),
                        threshold: num("Threshold").unwrap_or(2),
                        sub: sd.to_string(),
                        root,
                        idx: listed.iter().position(|l| l == sd),
                        app_or_mime: matches!(
                            get(sd, "Context"),
                            Some("Applications" | "MimeTypes")
                        ),
                        files: Default::default(),
                    });
                }
            }
        }
        let caches = roots.iter().map(|r| GtkCache::read(r)).collect();
        // QIconTheme::parents re-appends hicolor last (see `search`).
        let mut inherits = list("Inherits");
        inherits.retain(|t| t != "hicolor");
        std::rc::Rc::new(Theme {
            dirs,
            caches,
            inherits,
        })
    });
    THEMES.with(|c| c.borrow_mut().insert(name.to_string(), theme.clone()));
    theme
}

/// directoryMatchesSizeAndScale.
fn matches(d: &Dir, size: i32, scale: i32) -> bool {
    if d.scale != scale {
        return false;
    }
    match d.kind {
        Kind::Fixed => d.size == size,
        Kind::Scalable => d.min <= size && size <= d.max,
        Kind::Threshold => d.size - d.threshold <= size && size <= d.size + d.threshold,
    }
}

/// directorySizeDelta: positive when the directory is larger than `size` at `scale`.
fn delta(d: &Dir, size: i32, scale: i32) -> i32 {
    let size = size * scale;
    match d.kind {
        Kind::Fixed => d.size * d.scale - size,
        Kind::Scalable if size < d.min * d.scale => d.min * d.scale - size,
        Kind::Scalable if size > d.max * d.scale => size - d.max * d.scale,
        Kind::Scalable => 0,
        Kind::Threshold if size < (d.size - d.threshold) * d.scale => d.min * d.scale - size,
        Kind::Threshold if size > (d.size + d.threshold) * d.scale => size - d.max * d.scale,
        Kind::Threshold => 0,
    }
}

/// What QIconLoader::loadIcon found for a name, which QIcon::fromTheme keeps with the name.
#[derive(Clone)]
enum Found {
    /// QThemeIconInfo::entries: the files of the first theme that has the name, each with
    /// the index of its directory in the theme.
    Entries(std::rc::Rc<Theme>, Vec<(usize, PathBuf)>),
    /// A file that is not picked by size: one outside the themes, or KIconLoader's answer.
    File(PathBuf),
}

impl Found {
    /// QIconLoaderEngine::entryForSize's pick for `size` at the integer `scale`.
    fn at(&self, size: i32, scale: i32) -> &Path {
        let (t, entries) = match self {
            Found::File(f) => return f,
            Found::Entries(t, entries) => (t, entries),
        };
        if entries.len() <= 1 {
            return &entries[0].1;
        }
        let mut min_delta = i32::MIN;
        let mut closest = 0;
        for (k, (d, file)) in entries.iter().enumerate() {
            let d = &t.dirs[*d];
            if matches(d, size, scale) {
                return file;
            }
            let dv = delta(d, size, scale);
            // Downscaled icons win over upscaled ones.
            if (dv > min_delta && min_delta <= 0) || (dv > 0 && dv < min_delta.abs()) || dv == 0 {
                min_delta = dv;
                closest = k;
            }
        }
        &entries[closest].1
    }
}

/// QIcon's qtIconCache: what was found for each of the [`NAMES_KEPT`] names asked last,
/// with the count of asks at its last one. A name asked again is not searched again,
/// whatever its size and scale and whatever became of the themes' caches since.
#[derive(Default)]
struct Names {
    asks: u64,
    of: HashMap<String, (u64, Option<Found>)>,
}

/// QCache's default maxCost; an icon costs 1.
const NAMES_KEPT: usize = 100;

impl Names {
    fn get(&mut self, name: &str) -> Option<Option<Found>> {
        self.asks += 1;
        let (at, found) = self.of.get_mut(name)?;
        *at = self.asks;
        Some(found.clone())
    }

    fn keep(&mut self, name: &str, found: Option<Found>) {
        if self.of.len() >= NAMES_KEPT
            && let Some(oldest) =
                (self.of.iter().min_by_key(|(_, (at, _))| *at)).map(|(n, _)| n.clone())
        {
            self.of.remove(&oldest);
        }
        self.of.insert(name.to_string(), (self.asks, found));
    }
}

/// QIconLoader::findIconHelper's entries for `name` in one theme (png before svg, a png per
/// directory ahead of the previous ones; of a directory of the theme with a valid
/// `icon-theme.cache`, only the directories the cache names for `name`, in its order).
/// `size` is what KIconLoader picks by.
fn in_theme(
    t: &std::rc::Rc<Theme>,
    name: &str,
    size: i32,
    scale: i32,
    generic: bool,
    kde: bool,
) -> Option<Found> {
    if kde {
        return in_theme_kde(t, name, size, scale, generic).map(Found::File);
    }
    let (png, svg) = (format!("{name}.png"), format!("{name}.svg"));
    let mut entries: std::collections::VecDeque<(usize, PathBuf)> = Default::default();
    for (root, cache) in t.caches.iter().enumerate() {
        let of_root = || {
            t.dirs
                .iter()
                .enumerate()
                .filter(move |(_, d)| d.root == root)
        };
        let dirs: Vec<(usize, &Dir)> = match cache.as_ref().and_then(|c| c.lookup(name)) {
            Some(subs) => subs
                .iter()
                .filter_map(|s| of_root().find(|(_, d)| d.sub == *s))
                .collect(),
            None => of_root().collect(),
        };
        for (k, d) in dirs
            .into_iter()
            .filter(|(_, d)| !(generic && d.app_or_mime))
        {
            if d.has(&png) && d.path.join(&png).is_file() {
                entries.push_front((k, d.path.join(&png)));
            } else if d.has(&svg) && d.path.join(&svg).is_file() {
                entries.push_back((k, d.path.join(&svg)));
            }
        }
    }
    (!entries.is_empty()).then(|| Found::Entries(t.clone(), entries.into()))
}

/// plasma-integration's KIconEngine picks through KIconLoader: KIconTheme::iconPathByName
/// (one pass per extension, png first) for `size` at a devicePixelRatio of `scale` 120ths.
/// KIconTheme::iconPath takes a directory of that ratio rounded up as its `Scale` that
/// matches `size` (MatchExact), else the closest one of scale 1 (MatchBest) to `size` times
/// the ratio, cut to a whole number. Directories are in `Directories` order, theme roots
/// inner.
fn in_theme_kde(t: &Theme, name: &str, size: i32, scale: i32, generic: bool) -> Option<PathBuf> {
    let mut dirs: Vec<&Dir> = t
        .dirs
        .iter()
        .filter(|d| d.idx.is_some() && !(generic && d.app_or_mime))
        .collect();
    dirs.sort_by_key(|d| d.idx);
    let whole = (scale + 119) / 120;
    let px = (size as f64 * (scale as f64 / 120.0)) as i32;
    for ext in ["png", "svgz", "svg", "xpm"] {
        let file = format!("{name}.{ext}");
        let has = |d: &Dir| d.has(&file) && d.path.join(&file).is_file();
        let exact = |d: &&&Dir| {
            d.scale == whole
                && match d.kind {
                    Kind::Fixed => d.size == size,
                    Kind::Scalable => d.min <= size && size <= d.max,
                    Kind::Threshold => (d.size - size).abs() <= d.threshold,
                }
        };
        if let Some(d) = dirs
            .iter()
            .filter(|d| d.scale > 1)
            .filter(exact)
            .find(|d| has(d))
        {
            return Some(d.path.join(&file));
        }
        let size = px;
        let (mut path, mut delta) = (None, -i32::MAX);
        for d in dirs.iter().filter(|d| d.scale == 1) {
            // dw < 0: the icon would be upscaled.
            let dw = match d.kind {
                Kind::Fixed => d.size - size,
                Kind::Scalable if size < d.min => d.min - size,
                Kind::Scalable if size > d.max => d.max - size,
                Kind::Scalable => 0,
                Kind::Threshold if size < d.size - d.threshold => d.size - d.threshold - size,
                Kind::Threshold if size > d.size + d.threshold => d.size + d.threshold - size,
                Kind::Threshold => 0,
            };
            if dw.abs() >= delta.abs() && (dw < 0 || delta > 0) {
                continue;
            }
            if !has(d) {
                continue;
            }
            path = Some(d.path.join(&file));
            delta = dw;
            if dw == 0 {
                break;
            }
        }
        if path.is_some() {
            return path;
        }
    }
    None
}

/// One lookup's search inputs: the fallback theme every theme inherits after its own parents.
struct Query<'a> {
    name: &'a str,
    /// Of the request: only KIconLoader's search reads it.
    size: i32,
    /// The devicePixelRatio KIconLoader's search is for, in 120ths.
    scale: i32,
    /// The search is KIconLoader's (plasma-integration's KIconEngine), not QIconLoader's.
    kde: bool,
    generic: bool,
    fallback: &'a str,
}

fn search(theme: &str, q: &Query, seen: &mut Vec<String>) -> Option<Found> {
    if seen.iter().any(|s| s == theme) {
        return None;
    }
    seen.push(theme.to_string());
    let t = load_theme(theme)?;
    if let Some(p) = in_theme(&t, q.name, q.size, q.scale, q.generic, q.kde) {
        return Some(p);
    }
    // QIconTheme::parents: Inherits, the fallback theme, then hicolor.
    let parents = t.inherits.iter().map(String::as_str);
    for parent in parents.chain([q.fallback, "hicolor"]) {
        if !parent.is_empty()
            && let Some(p) = search(parent, q, seen)
        {
            return Some(p);
        }
    }
    None
}

/// findIconHelper with QIconLoader::FallBack: the theme chain, then the name cut at its last
/// dash (`a-b-c` -> `a-b` -> `a`) with a fresh visited list; cut names skip app/mime dirs.
fn search_dash(theme: &str, q: &Query, seen: &mut Vec<String>) -> Option<Found> {
    search(theme, q, seen).or_else(|| {
        let (name, _) = q.name.rsplit_once('-')?;
        let q = Query {
            name,
            generic: true,
            ..*q
        };
        search_dash(theme, &q, &mut Vec::new())
    })
}

/// QIconLoader::loadIcon: theme, fallback theme, then the unthemed fallback paths.
fn load_icon(name: &str, size: i32, scale: i32, kde: bool) -> Option<Found> {
    let mut seen = Vec::new();
    let (theme, fallback) = theme_names();
    let q = Query {
        name,
        size,
        scale,
        kde,
        generic: false,
        fallback: &fallback,
    };
    search_dash(&theme, &q, &mut seen)
        .or_else(|| search_dash(&fallback, &q, &mut seen))
        .or_else(|| {
            if kde {
                // plasma-integration's KIconEngine: KIconLoader always has hicolor, then
                // the "User" group (KIconLoader::iconPath).
                search("hicolor", &q, &mut seen)
                    .or_else(|| user_icon(name, &data_dirs()).map(Found::File))
            } else {
                fallback_icon(name, &fallback_dirs()).map(Found::File)
            }
        })
        .or_else(|| last_resort(&q, &mut seen))
}

/// Icon file for `name` (an icon name or absolute path) closest to `size` px.
pub fn lookup(name: &str, size: i32) -> Option<String> {
    lookup_scaled(name, size, 1)
}

/// [`lookup`] for a window whose devicePixelRatio rounds up to `scale`: QIconLoaderEngine
/// takes a directory of that `Scale` first, else the one closest to `size * scale` px.
pub fn lookup_scaled(name: &str, size: i32, scale: i32) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let file = |found: Option<Found>| Some(found?.at(size, scale).display().to_string());
    let kde = matches!(
        crate::colorscheme::platform_theme(),
        crate::colorscheme::PlatformTheme::KdePlugin
    );
    if name.starts_with('/') || kde {
        let key = (name.to_string(), size, scale);
        if let Some(r) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
            return r;
        }
        let r = if name.starts_with('/') {
            // QIcon::fromTheme on a path: the file itself, else the fallback lookup's
            // extensions.
            ["", ".png", ".svg", ".svgz", ".xpm"]
                .iter()
                .map(|x| format!("{name}{x}"))
                .find(|f| Path::new(f).is_file())
        } else {
            // KIconLoader searches by size.
            file(load_icon(name, size, 120, true))
        };
        CACHE.with(|c| c.borrow_mut().insert(key, r.clone()));
        return r;
    }
    lookup_loader(name, size, scale)
}

/// The file Qt's own QIconLoaderEngine draws a theme icon `name` from, whatever the platform
/// theme: [`lookup_scaled`] without plasma-integration, and with it the answer for a name
/// its KIconEngine does not have (QIconLoader::iconEngine: an engine of the platform theme
/// that is null gives way to QIconLoaderEngine).
pub fn lookup_loader(name: &str, size: i32, scale: i32) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let file = |found: Option<Found>| Some(found?.at(size, scale).display().to_string());
    if let Some(found) = FOUND.with_borrow_mut(|f| f.get(name)) {
        return file(found);
    }
    // The search loads themes and reads their caches: FOUND is not borrowed through it.
    let found = load_icon(name, size, 120, false);
    FOUND.with_borrow_mut(|f| f.keep(name, found.clone()));
    file(found)
}

/// The file plasma-integration's KIconEngine draws a theme icon `name` from for a side of
/// `size` in a window whose devicePixelRatio is `scale` 120ths
/// (KIconLoaderPrivate::findMatchingIconWithGenericFallbacks).
pub fn lookup_kde(name: &str, size: i32, scale: i32) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    // No other key has a negative scale.
    let key = (name.to_string(), size, -scale);
    if let Some(r) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return r;
    }
    let r = load_icon(name, size, scale, true).map(|f| f.at(size, 1).display().to_string());
    CACHE.with(|c| c.borrow_mut().insert(key, r.clone()));
    r
}

/// QIconLoaderEngine::actualSize for a `rw`x`rh` request whose entry is `file`, a path
/// [`lookup`] answered: the request itself from a Scalable directory, else a square of the
/// directory's size, or of the request's shorter side when that is less. `None` for a file
/// outside the theme directories (a Fallback entry: QIcon(file) answers).
pub fn loader_size(file: &str, rw: i32, rh: i32) -> Option<(i32, i32)> {
    let dir = Path::new(file).parent()?;
    let (kind, size) = THEMES.with(|c| {
        c.borrow()
            .values()
            .flatten()
            .flat_map(|t| &t.dirs)
            .find(|d| d.path == dir)
            .map(|d| (d.kind, d.size * d.scale))
    })?;
    Some(match kind {
        Kind::Scalable => (rw, rh),
        _ => {
            let side = size.min(rw.min(rh));
            (side, side)
        }
    })
}

/// KIconLoader::global()->theme()->followsColorScheme(): KIconLoaderPrivate::createIconImage
/// recolors every SVG it loads when the user's icon theme (not the theme the file comes from)
/// sets `FollowsColorScheme=true`, so hicolor app icons with ColorScheme-* classes follow too.
pub fn theme_follows_color_scheme() -> bool {
    thread_local! {
        static FOLLOWS: RefCell<HashMap<String, bool>> = Default::default();
    }
    let (theme, fallback) = theme_names();
    FOLLOWS.with(|c| {
        *c.borrow_mut().entry(theme.clone()).or_insert_with(|| {
            // KIconLoader falls back to the default theme when the configured one is missing.
            [theme.as_str(), fallback.as_str()]
                .iter()
                .find_map(|t| {
                    bases()
                        .into_iter()
                        .find_map(|b| std::fs::read_to_string(b.join(t).join("index.theme")).ok())
                })
                .and_then(|t| crate::ini::value(&t, "Icon Theme", "FollowsColorScheme"))
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
        })
    })
}

/// Resolve an `image://icon/<name>[?fallback=<name>][?path=<dir>]` URL to a file for `size`
/// where QGuiApplication::devicePixelRatio() is `scale`: IconImageProvider::requestPixmap asks
/// `QIcon::pixmap(w, h)`, which takes that ratio (the highest wl_output.scale of all outputs),
/// not the window's.
///
/// With `?path=` (an SNI item's `IconThemePath`), `<dir>/<name>.{png,svg,xpm}` is checked at
/// once; a deeper theme tree is searched on a pool worker instead of the caller's thread
/// ([`own_theme_icon`]). While that search runs the URL resolves to `None` and
/// [`pending`] is true; `ready` runs on the worker once its result is cached.
pub fn resolve_url(
    url: &str,
    size: i32,
    scale: i32,
    ready: impl FnOnce() + Send + 'static,
) -> Option<String> {
    let rest = url.strip_prefix("image://icon/")?;
    let (rest, fallback) = match rest.split_once("?fallback=") {
        Some((n, f)) => (n, Some(f)),
        None => (rest, None),
    };
    let (name, path) = match rest.split_once("?path=") {
        Some((n, p)) => (n, Some(p)),
        None => (rest, None),
    };
    // IconImageProvider with `?path=`: the item's own theme directory is searched first.
    if let Some(p) = path {
        match own_theme_icon(p, name, ready) {
            Own::Found(f) => return Some(f),
            Own::Pending => return None,
            Own::Missing => {}
        }
    }
    lookup_scaled(name, size, scale)
        .or_else(|| fallback.and_then(|f| lookup_scaled(f, size, scale)))
}

/// Whether `url`'s own-theme (`?path=`) search is still running on a worker.
pub fn pending(url: &str) -> bool {
    let Some((name, path)) = url
        .strip_prefix("image://icon/")
        .map(|r| r.split_once("?fallback=").map_or(r, |(n, _)| n))
        .and_then(|r| r.split_once("?path="))
    else {
        return false;
    };
    let key = (path.to_string(), name.to_string());
    matches!(
        own_cache().lock().unwrap().get(&key),
        Some(OwnEntry::Pending(_))
    )
}

enum Own {
    Found(String),
    Missing,
    Pending,
}

type Ready = Box<dyn FnOnce() + Send>;

enum OwnEntry {
    /// Search running; requesters to tell when it lands.
    Pending(Vec<Ready>),
    Done(Option<String>),
}

/// The item whose `IconThemePath` is `dir` says its icons changed (it registered, or sent
/// `NewIcon` or `NewIconThemePath`): the names not found under `dir` are searched again when
/// next asked for (the app may have written its icon since). Nothing else forgets a miss: no
/// clock expires it.
pub fn own_theme_changed(dir: &str) {
    own_cache()
        .lock()
        .unwrap()
        .retain(|(d, _), e| d != dir || !matches!(e, OwnEntry::Done(None)));
}
const OWN_CACHE_MAX: usize = 512;
/// Directory entries one search reads at most (a huge tree costs a bounded walk).
const OWN_WALK_MAX: usize = 20_000;

fn own_cache() -> &'static std::sync::Mutex<HashMap<(String, String), OwnEntry>> {
    static C: std::sync::OnceLock<std::sync::Mutex<HashMap<(String, String), OwnEntry>>> =
        std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

/// An SNI `IconThemePath` comes from any D-Bus client: only an absolute directory that is not
/// the filesystem root or a pseudo filesystem is searched.
fn own_root_ok(dir: &Path) -> bool {
    dir.is_absolute()
        && dir.canonicalize().is_ok_and(|c| {
            c.is_dir()
                && c != Path::new("/")
                && !["/proc", "/sys", "/dev"].iter().any(|r| c.starts_with(r))
        })
}

fn own_theme_icon(dir: &str, name: &str, ready: impl FnOnce() + Send + 'static) -> Own {
    if dir.is_empty() || name.is_empty() || name.contains('/') {
        return Own::Missing;
    }
    if let Some(f) = ["png", "svg", "xpm"]
        .iter()
        .map(|e| format!("{dir}/{name}.{e}"))
        .find(|f| Path::new(f).is_file())
    {
        return Own::Found(f);
    }
    let key = (dir.to_string(), name.to_string());
    let mut c = own_cache().lock().unwrap();
    match c.get_mut(&key) {
        Some(OwnEntry::Done(Some(f))) => return Own::Found(f.clone()),
        Some(OwnEntry::Done(None)) => return Own::Missing,
        Some(OwnEntry::Pending(w)) => {
            w.push(Box::new(ready));
            return Own::Pending;
        }
        _ => {}
    }
    if !own_root_ok(Path::new(dir)) {
        c.insert(key, OwnEntry::Done(None));
        return Own::Missing;
    }
    if c.len() >= OWN_CACHE_MAX {
        c.retain(|_, e| matches!(e, OwnEntry::Pending(_)));
    }
    c.insert(key.clone(), OwnEntry::Pending(vec![Box::new(ready)]));
    drop(c);
    workpool::run(move || {
        let mut found = walk_icons(Path::new(&key.0), &key.1);
        found.sort();
        let r = found.pop();
        let waiting = match own_cache().lock().unwrap().insert(key, OwnEntry::Done(r)) {
            Some(OwnEntry::Pending(w)) => w,
            _ => vec![],
        };
        for f in waiting {
            f();
        }
    });
    Own::Pending
}

/// `<dir>/**/<name>.{png,svg}` (a theme rooted at an SNI `IconThemePath`), at most four
/// levels deep. Symlinked directories are not entered (a link to `/` or a loop would make it
/// a filesystem walk); symlinked files count.
fn walk_icons(dir: &Path, name: &str) -> Vec<String> {
    let mut out = vec![];
    let mut seen = 0;
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            seen += 1;
            if seen > OWN_WALK_MAX {
                return out;
            }
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                if depth < 4 {
                    stack.push((p, depth + 1));
                }
            } else if p.file_stem().is_some_and(|s| s == name)
                && p.extension().is_some_and(|x| x == "png" || x == "svg")
            {
                out.push(p.to_string_lossy().into_owned());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dirs_deduped_like_qstandardpaths() {
        assert_eq!(
            clean_data_dirs("/a/share:/usr/share/:rel:/a/share::/usr//share:/b"),
            ["/a/share", "/usr/share", "/b"]
        );
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }

    #[test]
    fn directory_listing_is_two_allocations_and_finds_what_a_set_finds() {
        let all = [
            "b.svg",
            "a.png",
            "a.svg",
            "",
            "ab",
            "z-long-name-symbolic.svg",
            "é.png",
        ];
        let n = Listing::of(all.iter().map(Into::into).collect());
        assert_eq!(n.ends.len(), all.len());
        assert_eq!(n.bytes.len(), all.iter().map(|a| a.len()).sum::<usize>());
        for a in all {
            assert!(n.contains(a), "{a}");
        }
        for a in ["a", "a.pn", "a.png2", "b", "c.svg", "z", "é"] {
            assert!(!n.contains(a), "{a}");
        }
        assert!(!Listing::default().contains("a.png"));
        assert!(!Listing::default().contains(""));

        let d = std::env::temp_dir().join(format!("qml-icons-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        touch(&d.join("x.svg"));
        touch(&d.join("y.png"));
        let dir = Dir {
            path: d.clone(),
            size: 16,
            scale: 1,
            kind: Kind::Fixed,
            min: 16,
            max: 16,
            threshold: 2,
            sub: String::new(),
            root: 0,
            idx: None,
            app_or_mime: false,
            files: Default::default(),
        };
        assert!(dir.has("x.svg") && dir.has("y.png"));
        assert!(!dir.has("x.png") && !dir.has("y"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// An `icon-theme.cache` of the icon `x` in the directory `d`: header, hash table of one
    /// bucket, the icon, the list of directories, the icon's list of images.
    fn gtk_cache() -> Vec<u8> {
        let words: [u32; 15] = [
            0x0001_0000,
            12,
            36,
            1,
            20,
            u32::MAX,
            32,
            48,
            u32::from_be_bytes(*b"x\0\0\0"),
            1,
            44,
            u32::from_be_bytes(*b"d\0\0\0"),
            1,
            0,
            0,
        ];
        words.iter().flat_map(|w| w.to_be_bytes()).collect()
    }

    fn directories(data: Vec<u8>, name: &str) -> Option<Vec<String>> {
        let c = GtkCache {
            data,
            valid: std::cell::Cell::new(true),
        };
        let found = c.lookup(name)?;
        Some(found.iter().map(|d| d.to_string()).collect())
    }

    #[test]
    fn gtk_cache_names_the_directories_of_an_icon() {
        assert_eq!(directories(gtk_cache(), "x"), Some(vec!["d".to_string()]));
        assert_eq!(directories(gtk_cache(), "y"), Some(vec![]));
    }

    /// QIconCacheGtkReader::lookup: `listOffset + 4 + 8 * listLen > m_size` and `o >= m_size`
    /// make the cache invalid.
    #[test]
    fn gtk_cache_is_invalid_past_its_end() {
        let mut cut = gtk_cache();
        cut.truncate(56);
        assert_eq!(directories(cut, "x"), None);
        let mut name_at_end = gtk_cache();
        name_at_end[40..44].copy_from_slice(&60u32.to_be_bytes());
        assert_eq!(directories(name_at_end, "x"), None);
    }

    #[test]
    fn kiconloader_prefers_downscaling() {
        let t = std::env::temp_dir().join(format!("ri-kicons-{}", std::process::id()));
        // breeze mimetypes: 16 Fixed, 22 Scalable 22-24, 32 Fixed, 64 Scalable 64-256.
        let spec = [
            ("16", Kind::Fixed, 16, 16, 16),
            ("22", Kind::Scalable, 22, 22, 24),
            ("32", Kind::Fixed, 32, 32, 32),
            ("64", Kind::Scalable, 64, 64, 256),
        ];
        let dirs = spec
            .iter()
            .enumerate()
            .map(|(idx, &(n, kind, size, min, max))| {
                touch(&t.join(n).join("audio-x-generic.svg"));
                Dir {
                    path: t.join(n),
                    size,
                    scale: 1,
                    kind,
                    min,
                    max,
                    threshold: 2,
                    sub: n.to_string(),
                    root: 0,
                    idx: Some(idx),
                    app_or_mime: true,
                    files: Default::default(),
                }
            })
            .collect();
        let theme = std::rc::Rc::new(Theme {
            dirs,
            caches: vec![None],
            inherits: vec![],
        });
        let pick = |size| in_theme_kde(&theme, "audio-x-generic", size, 120, false);
        // Upscaled 32 (dw -2) loses to downscaled 64 (dw +30); QIconLoader takes 22.
        assert_eq!(pick(34), Some(t.join("64/audio-x-generic.svg")));
        assert_eq!(pick(24), Some(t.join("22/audio-x-generic.svg")));
        assert_eq!(pick(20), Some(t.join("22/audio-x-generic.svg")));
        assert_eq!(pick(32), Some(t.join("32/audio-x-generic.svg")));
        assert!(in_theme(&theme, "audio-x-generic", 34, 120, false, false).is_some());
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn absolute_path_without_extension_tries_qt_fallback_extensions() {
        let t = std::env::temp_dir().join(format!("ri-icon-ext-{}", std::process::id()));
        // Kirigami.Icon { source: "${Looks.iconsPath}/window-shield" } (waffle polkit dialog).
        touch(&t.join("window-shield.svg"));
        touch(&t.join("both.png"));
        touch(&t.join("both.svg"));
        touch(&t.join("zipped.svgz"));
        touch(&t.join("exact"));
        let at = |n: &str| lookup(&t.join(n).display().to_string(), 26);
        let file = |n: &str| Some(t.join(n).display().to_string());
        assert_eq!(at("window-shield"), file("window-shield.svg"));
        assert_eq!(at("both"), file("both.png"));
        assert_eq!(at("zipped"), file("zipped.svgz"));
        assert_eq!(at("exact"), file("exact"));
        assert_eq!(at("missing"), None);
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn user_group_icons_and_pixmaps() {
        let t = std::env::temp_dir().join(format!("ri-icons-{}", std::process::id()));
        let (home, sys) = (t.join("home"), t.join("sys"));
        touch(&home.join("icons/impulse-shell.svg"));
        touch(&sys.join("pixmaps/logo.png"));
        touch(&sys.join("icons/logo.svg"));
        let data = [home.clone(), sys.clone()];
        assert_eq!(
            user_icon("impulse-shell", &data),
            Some(home.join("icons/impulse-shell.svg"))
        );
        // svg is tried in every search path before png.
        assert_eq!(user_icon("logo", &data), Some(sys.join("icons/logo.svg")));
        assert_eq!(user_icon("missing", &data), None);
        let pix = [home.join("pixmaps"), sys.join("pixmaps")];
        assert_eq!(
            fallback_icon("logo", &pix),
            Some(sys.join("pixmaps/logo.png"))
        );
        // Unthemed Qt lookup never looks in the bare icons dir.
        assert_eq!(fallback_icon("impulse-shell", &pix), None);
        std::fs::remove_dir_all(&t).unwrap();
    }

    fn wait_ready(rx: &std::sync::mpsc::Receiver<()>) {
        rx.recv().expect("own-theme search reported back");
    }

    #[test]
    fn own_theme_tree_is_searched_off_thread() {
        let t = std::env::temp_dir().join(format!("ri-own-async-{}", std::process::id()));
        touch(&t.join("hicolor/22x22/apps/tray-own-async.png"));
        let url = format!("image://icon/tray-own-async?path={}", t.display());
        let (tx, rx) = std::sync::mpsc::channel();
        // The caller (the render thread) never walks the tree itself.
        assert_eq!(resolve_url(&url, 22, 1, move || tx.send(()).unwrap()), None);
        wait_ready(&rx);
        assert!(!pending(&url));
        // Cached: answered at once, no further search.
        assert_eq!(
            resolve_url(&url, 22, 1, || panic!("cached result searched again")),
            Some(
                t.join("hicolor/22x22/apps/tray-own-async.png")
                    .display()
                    .to_string()
            )
        );
        std::fs::remove_dir_all(&t).unwrap();
    }

    /// A name not found stays not found, however long ago it was searched, until the item
    /// says its icons changed; then it is searched again.
    #[test]
    fn own_theme_miss_is_searched_again_when_the_item_says_so() {
        let t = std::env::temp_dir().join(format!("ri-own-later-{}", std::process::id()));
        std::fs::create_dir_all(t.join("hicolor/22x22/apps")).unwrap();
        let dir = t.display().to_string();
        let url = format!("image://icon/tray-own-later?path={dir}");
        let (tx, rx) = std::sync::mpsc::channel();
        assert_eq!(resolve_url(&url, 22, 1, move || tx.send(()).unwrap()), None);
        wait_ready(&rx);
        // The app writes its icon after the search.
        let icon = t.join("hicolor/22x22/apps/tray-own-later.png");
        touch(&icon);
        assert_eq!(
            resolve_url(&url, 22, 1, || panic!("a miss searched again unasked")),
            None
        );
        // Another item's theme directory is not this one's.
        own_theme_changed(&format!("{dir}-other"));
        assert_eq!(
            resolve_url(&url, 22, 1, || panic!("a miss searched again unasked")),
            None
        );
        own_theme_changed(&dir);
        let (tx, rx) = std::sync::mpsc::channel();
        assert_eq!(resolve_url(&url, 22, 1, move || tx.send(()).unwrap()), None);
        wait_ready(&rx);
        assert_eq!(
            resolve_url(&url, 22, 1, || panic!("cached result searched again")),
            Some(icon.display().to_string())
        );
        // What was found is kept.
        own_theme_changed(&dir);
        assert_eq!(
            resolve_url(&url, 22, 1, || panic!("cached result searched again")),
            Some(icon.display().to_string())
        );
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn own_theme_top_level_icon_is_immediate() {
        let t = std::env::temp_dir().join(format!("ri-own-top-{}", std::process::id()));
        touch(&t.join("tray-own-top.svg"));
        let url = format!("image://icon/tray-own-top?path={}", t.display());
        assert_eq!(
            resolve_url(&url, 22, 1, || panic!(
                "top-level icon searched on a worker"
            )),
            Some(t.join("tray-own-top.svg").display().to_string())
        );
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn own_theme_walk_skips_symlinked_directories() {
        let t = std::env::temp_dir().join(format!("ri-own-link-{}", std::process::id()));
        let outside = t.join("outside");
        touch(&outside.join("tray-own-link.png"));
        std::fs::create_dir_all(t.join("theme")).unwrap();
        std::os::unix::fs::symlink(&outside, t.join("theme/elsewhere")).unwrap();
        // A loop must not make the walk revisit the tree either.
        std::os::unix::fs::symlink(t.join("theme"), t.join("theme/loop")).unwrap();
        let url = format!(
            "image://icon/tray-own-link?path={}",
            t.join("theme").display()
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let first = resolve_url(&url, 22, 1, move || tx.send(()).unwrap());
        if pending(&url) {
            wait_ready(&rx);
        }
        let own = resolve_url(&url, 22, 1, || {});
        for r in [first, own] {
            assert!(
                r.as_deref()
                    .is_none_or(|f| !f.starts_with(&*t.to_string_lossy())),
                "icon found through a symlinked directory: {r:?}"
            );
        }
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn the_names_asked_last_are_kept_as_qcache_keeps_them() {
        let mut names = Names::default();
        let file = |n: usize| Some(Found::File(PathBuf::from(format!("/{n}"))));
        for n in 0..NAMES_KEPT {
            assert!(names.get(&n.to_string()).is_none());
            names.keep(&n.to_string(), file(n));
        }
        // QCache::object moves the name to the front: 0 outlives 1.
        assert!(names.get("0").is_some());
        assert!(names.get("new").is_none());
        names.keep("new", None);
        assert_eq!(names.of.len(), NAMES_KEPT);
        assert!(names.get("1").is_none());
        assert!(matches!(names.get("0"), Some(Some(Found::File(f))) if f == Path::new("/0")));
        // A name that was not found is kept too.
        assert!(matches!(names.get("new"), Some(None)));
    }

    #[test]
    fn own_theme_rejects_system_roots() {
        let t = std::env::temp_dir().join(format!("ri-own-root-{}", std::process::id()));
        std::fs::create_dir_all(&t).unwrap();
        std::os::unix::fs::symlink("/", t.join("root")).unwrap();
        let link = t.join("root").display().to_string();
        for dir in [
            "/",
            "/proc",
            "/sys/class",
            "/dev",
            link.as_str(),
            "relative/dir",
        ] {
            let url = format!("image://icon/no-such-tray-icon-xyz?path={dir}");
            let r = resolve_url(&url, 22, 1, || {});
            // No search is started at all: nothing pending, the answer is immediate.
            assert!(!pending(&url), "{dir} searched");
            assert_eq!(r, None, "{dir}");
        }
        std::fs::remove_dir_all(&t).unwrap();
    }
}
