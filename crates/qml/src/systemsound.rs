//! The `SystemSound` singleton of module `Impulse` (`Impulse/systemsound.qmltypes`):
//! `play(theme, name)` plays the theme's sound once, detached, never through a shell. The
//! file is found as the freedesktop Sound Theme Specification says
//! (<https://specifications.freedesktop.org/sound-theme-spec/latest/>).

use crate::eval::{Interp, R};
use crate::value::Value;
use std::path::{Path, PathBuf};

/// The QML name of the singleton.
pub const NAME: &str = "SystemSound";

/// The theme every lookup ends in before unthemed sounds (spec, "Directory Layout").
const DEFAULT_THEME: &str = "freedesktop";

/// The output profile sounds are played for; the spec's last resort as well.
const PROFILE: &str = "stereo";

/// What a lookup needs of the environment: the base directories, in order, and the
/// `LC_MESSAGES` locale.
pub struct Lookup {
    pub bases: Vec<PathBuf>,
    pub locale: String,
}

impl Lookup {
    /// `$XDG_DATA_HOME/sounds`, then each `$XDG_DATA_DIRS/sounds` (XDG Base Directory
    /// defaults), and the locale of `LC_ALL`, `LC_MESSAGES` or `LANG`, whichever is set
    /// first, as setlocale(3) picks it.
    pub fn from_env(env: &dyn Fn(&str) -> Option<String>) -> Lookup {
        let set = |k: &str| env(k).filter(|v| !v.is_empty());
        let home = set("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| set("HOME").map(|h| Path::new(&h).join(".local/share")));
        let dirs = set("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        let bases = home
            .into_iter()
            .chain(dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from))
            .map(|d| d.join("sounds"))
            .collect();
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|k| set(k))
            .unwrap_or_default();
        Lookup { bases, locale }
    }

    /// The file of sound `name` in `theme`: the theme itself, the themes it inherits
    /// (depth first, each once), then `freedesktop`, then unthemed. `None` when no theme has
    /// it or the first one that does has it `.disabled`.
    pub fn sound_file(&self, theme: &str, name: &str) -> Option<PathBuf> {
        let mut order = Vec::new();
        self.inheritance(theme, &mut order);
        self.inheritance(DEFAULT_THEME, &mut order);
        let themed = order.iter().find_map(|t| self.in_theme(Some(t), name));
        match themed.or_else(|| self.in_theme(None, name))? {
            Found::File(f) => Some(f),
            Found::Disabled => None,
        }
    }

    /// `theme` and the themes it inherits, appended to `order` unless already there.
    fn inheritance(&self, theme: &str, order: &mut Vec<Theme>) {
        // A theme's name is the name of its directory (spec): nothing with a path in it.
        if theme.is_empty() || theme.contains('/') || order.iter().any(|t| t.name == theme) {
            return;
        }
        let Some(t) = self.theme(theme) else {
            return;
        };
        let parents = t.inherits.clone();
        order.push(t);
        for p in parents {
            self.inheritance(&p, order);
        }
    }

    /// The theme described by the first `index.theme` found over the base directories.
    fn theme(&self, name: &str) -> Option<Theme> {
        let text = self
            .bases
            .iter()
            .find_map(|b| std::fs::read_to_string(b.join(name).join("index.theme")).ok())?;
        Some(Theme::parse(name, &text))
    }

    /// The spec's `LookupSound` for one theme (`None`: unthemed, files directly in a base
    /// directory): profile, then subdirectory, base directory, name, locale and extension.
    fn in_theme(&self, theme: Option<&Theme>, name: &str) -> Option<Found> {
        let unthemed = [(String::new(), String::new())];
        let dirs = theme.map_or(&unthemed[..], |t| &t.dirs);
        for profile in [PROFILE, ""] {
            for (subdir, _) in dirs.iter().filter(|(_, p)| p == profile) {
                for base in &self.bases {
                    let dir = base.join(theme.map_or("", |t| &t.name)).join(subdir);
                    for name in truncations(name, '-') {
                        for locale in locales(&self.locale) {
                            for ext in ["disabled", "oga", "ogg", "wav"] {
                                let f = dir.join(locale).join(format!("{name}.{ext}"));
                                if f.is_file() {
                                    return Some(match ext {
                                        "disabled" => Found::Disabled,
                                        _ => Found::File(f),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

enum Found {
    File(PathBuf),
    /// A `.disabled` file: the lookup ends, the sound is not available.
    Disabled,
}

/// A theme's `index.theme`: its subdirectories with their output profiles and the themes it
/// inherits.
struct Theme {
    name: String,
    /// (subdirectory, `OutputProfile`), in `Directories` order.
    dirs: Vec<(String, String)>,
    inherits: Vec<String>,
}

impl Theme {
    /// The `Sound Theme` group's `Directories` and `Inherits` (comma-separated lists) and
    /// each listed directory's `OutputProfile`; the file format is the desktop entry one.
    fn parse(name: &str, text: &str) -> Theme {
        let mut group = "";
        let mut keys: Vec<(&str, &str, &str)> = Vec::new();
        for line in text.lines().map(str::trim) {
            if let Some(g) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                group = g;
            } else if !line.starts_with('#')
                && let Some((k, v)) = line.split_once('=')
            {
                keys.push((group, k.trim_end(), v.trim_start()));
            }
        }
        let value = |g: &str, k: &str| {
            let found = keys.iter().find(|(kg, kk, _)| *kg == g && *kk == k);
            found.map_or("", |(_, _, v)| *v)
        };
        let list = |v: &str| -> Vec<String> {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        };
        let dirs = list(value("Sound Theme", "Directories"))
            .into_iter()
            // A subdirectory stays inside its theme.
            .filter(|d| !Path::new(d).is_absolute() && !d.split('/').any(|c| c == ".."))
            .map(|d| {
                let profile = value(&d, "OutputProfile").to_owned();
                (d, profile)
            })
            .collect();
        Theme {
            name: name.to_owned(),
            dirs,
            inherits: list(value("Sound Theme", "Inherits")),
        }
    }
}

/// `name`, then `name` cut at its last `sep`, and so on while it has one.
fn truncations(name: &str, sep: char) -> Vec<&str> {
    let mut v = vec![name];
    let mut cur = name;
    while let Some(i) = cur.rfind(sep) {
        cur = &cur[..i];
        v.push(cur);
    }
    v
}

/// The locale directories to try: the locale, it cut at `@`, it cut at `_`, `C`, none.
fn locales(locale: &str) -> Vec<&str> {
    let cut = |c: char| locale.split_once(c).map_or(locale, |(head, _)| head);
    let mut v: Vec<&str> = Vec::new();
    for l in [locale, cut('@'), cut('_'), "C"] {
        // A locale names one directory.
        if !l.is_empty() && !l.contains('/') && !v.contains(&l) {
            v.push(l);
        }
    }
    v.push("");
    v
}

/// The player for `file`: PipeWire's `pw-play`, else `ffplay`.
pub fn player_argv(file: &Path, path_var: Option<&std::ffi::OsStr>) -> Option<Vec<String>> {
    let f = file.to_string_lossy().into_owned();
    if on_path("pw-play", path_var) {
        Some(vec!["pw-play".into(), f])
    } else if on_path("ffplay", path_var) {
        Some(vec![
            "ffplay".into(),
            "-nodisp".into(),
            "-autoexit".into(),
            f,
        ])
    } else {
        None
    }
}

fn on_path(name: &str, path_var: Option<&std::ffi::OsStr>) -> bool {
    path_var.is_some_and(|p| crate::osutil::find_executable(name, p).is_some())
}

/// `play(theme, name)`: the theme's file for the sound, once; nothing when no theme has it
/// or no player is installed.
fn play(_: &mut Interp, _: &Value, args: &[Value]) -> R<Value> {
    let s = |i: usize| args.get(i).map(|v| v.to_js_string()).unwrap_or_default();
    let (theme, name) = (s(0), s(1));
    if crate::impulse::hooked(NAME, "play", &[&theme, &name]) {
        return Ok(Value::Undefined);
    }
    let pv = std::env::var_os("PATH");
    let file = Lookup::from_env(&|k| std::env::var(k).ok()).sound_file(&theme, &name);
    if let Some(argv) = file.and_then(|f| player_argv(&f, pv.as_deref())) {
        crate::native::detach_program(&argv);
    }
    Ok(Value::Undefined)
}

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: NAME,
    methods: &[("play", play)],
    init: None,
};
