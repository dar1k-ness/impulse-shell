//! The wallpaper picker's actions (services/Wallpick.qml's `WallpickRun`, and the
//! `impulse-shell wallpick <action>` command), run in-process: the engine is the
//! `wallpick` crate; applying a pick is the in-process `colorgen::switchwall` and edits of
//! the shell config go through [`crate::shell_config`], as for every other native script.
//! Each line on stdout is a status the settings pages show while the job runs.
use crate::jobs::{Job, Pipes};
use crate::runtime::World;
use crate::svc::Stop;
use crate::value::Value;
use colorgen::switchwall::Io;
use serde_json::{Value as J, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What a run reads from the session: kept apart so tests can point it at a temp home.
pub struct Ctx {
    pub home: PathBuf,
    pub config_home: PathBuf,
    pub state_home: PathBuf,
    pub cache_home: PathBuf,
    pub pictures: PathBuf,
    /// `<shell>/scripts/colors`, switchwall's own directory.
    pub colors_dir: PathBuf,
    pub sys: Arc<dyn colorgen::sys::Sys>,
    /// Logical size of the screen the desktop clock is laid out on.
    pub screen: (u32, u32),
}

impl Ctx {
    fn from_env(root: &Path, screen: (u32, u32)) -> Ctx {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let xdg = |k: &str, d: &str| match std::env::var(k) {
            Ok(v) if v.starts_with('/') => PathBuf::from(v),
            _ => home.join(d),
        };
        Ctx {
            config_home: xdg("XDG_CONFIG_HOME", ".config"),
            state_home: xdg("XDG_STATE_HOME", ".local/state"),
            cache_home: xdg("XDG_CACHE_HOME", ".cache"),
            pictures: crate::jobs_wall_gcloud_trans::user_pictures_dir().into(),
            colors_dir: root.join("scripts/colors"),
            sys: crate::shell_config::sys(),
            screen,
            home,
        }
    }

    fn config_file(&self) -> PathBuf {
        self.config_home.join("impulse-shell/config.json")
    }

    /// The shell's layout (`layout_model::default_path` under this config home).
    fn layout_file(&self) -> PathBuf {
        self.config_home.join("impulse-shell/layout.toml")
    }

    fn dirs(&self) -> wallpick::Dirs {
        wallpick::Dirs {
            state: colorgen::fsutil::state_dir(&self.state_home).join("user/wallpick"),
            cache: colorgen::fsutil::cache_dir(&self.cache_home).join("wallpick"),
            downloads: self.pictures.join("Wallpapers"),
        }
    }

    /// The user's config.json; none yet reads as empty, a broken one is an error rather than
    /// silently picking with the defaults.
    fn config(&self) -> Result<J, String> {
        let file = self.config_file();
        match std::fs::read_to_string(&file) {
            Ok(t) => serde_json::from_str(&t)
                .map_err(|e| format!("Could not read {}: {e}", file.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(J::Null),
            Err(e) => Err(format!("Could not read {}: {e}", file.display())),
        }
    }
}

/// The action services/Wallpick.qml's `run()` gives `proc`, a `WallpickRun`: its `args`.
fn args(w: &mut World, p: usize) -> Option<Vec<String>> {
    match w.read(p, "args") {
        Some(Value::Obj(o)) => Some(
            o.borrow()
                .array
                .as_ref()?
                .iter()
                .map(|v| v.to_js_string().to_string())
                .collect(),
        ),
        _ => None,
    }
}

/// `Impulse/WallpickRun` (services/Wallpick.qml's `proc`): the action its `args` name, run
/// in-process.
pub(crate) fn job(w: &mut World, p: usize) -> Option<Job> {
    let action = Action::parse(&args(w, p)?)?;
    let screen = screen_size(w);
    let ctx = Ctx::from_env(w.root_dir.clone()?.as_ref(), screen);
    Some(Box::new(move |io: &Pipes, stop: &Arc<Stop>| {
        let cancel = wallpick::Cancel::default();
        let c = cancel.clone();
        let _cancels = stop.on_stop(move || c.cancel());
        let out = |s: String| io.out(s);
        let err = |s: String| io.err(s);
        run(
            &ctx,
            action,
            &cancel,
            Io {
                out: &out,
                err: &err,
            },
        )
    }))
}

/// `impulse-shell wallpick <action>` outside a running shell (scripts, keybinds).
pub fn cli(args: &[String], root: &Path) -> i32 {
    let Some(action) = Action::parse(args) else {
        elog::eprintln!(
            "usage: impulse-shell wallpick pick [--desktop | --words TEXT] | like | dislike [--next] | revert | keep | style | reset-taste | arrange LEFT CENTER RIGHT"
        );
        return 2;
    };
    let ctx = Ctx::from_env(root, (1920, 1080));
    run(&ctx, action, &wallpick::Cancel::default(), Io::STD)
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Pick {
        by: By,
    },
    /// Puts back the look the desktop had before the picks since the last [`Action::Keep`].
    Revert,
    /// The picked look stays: there is nothing to put back any more.
    Keep,
    Like,
    Dislike {
        next: bool,
    },
    Style,
    ResetTaste,
    /// The original's bar groups per part (left, center, right), as `bar.layout` holds them.
    Arrange {
        parts: [Vec<String>; 3],
    },
}

/// What a pick goes by (`wallpick::By`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum By {
    /// The settings' text preferences: the pick of a script or a keybind, which nobody reviews.
    Prefs,
    /// The wallpaper on the desktop.
    Desktop,
    /// The user's own words, in any language.
    Words(String),
    /// What the pick before went by.
    Same,
}

impl Action {
    pub fn parse(args: &[String]) -> Option<Action> {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        Some(match a.as_slice() {
            ["pick"] => Action::Pick { by: By::Prefs },
            ["pick", "--desktop"] => Action::Pick { by: By::Desktop },
            ["pick", "--words", words] if !words.trim().is_empty() => Action::Pick {
                by: By::Words(words.trim().to_string()),
            },
            ["revert"] => Action::Revert,
            ["keep"] => Action::Keep,
            ["like"] => Action::Like,
            ["dislike"] => Action::Dislike { next: false },
            ["dislike", "--next"] => Action::Dislike { next: true },
            ["style"] => Action::Style,
            ["reset-taste"] => Action::ResetTaste,
            ["arrange", left, center, right] => {
                let list = |s: &str| -> Vec<String> {
                    s.split(',')
                        .filter(|n| !n.is_empty())
                        .map(str::to_string)
                        .collect()
                };
                Action::Arrange {
                    parts: [list(left), list(center), list(right)],
                }
            }
            _ => return None,
        })
    }
}

/// The primary output's logical size, else 1920x1080.
fn screen_size(w: &mut World) -> (u32, u32) {
    let first = w
        .singletons
        .get("Rustshell")
        .copied()
        .and_then(|qs| match w.read(qs, "screens") {
            Some(Value::Obj(o)) => o.borrow().array.as_ref()?.first().cloned(),
            _ => None,
        });
    let mut dim = |k: &str| match first {
        Some(Value::Item(s)) => match w.read(s, k) {
            Some(Value::Num(n)) if n >= 1.0 => Some(n as u32),
            _ => None,
        },
        _ => None,
    };
    dim("width").zip(dim("height")).unwrap_or((1920, 1080))
}

fn say(io: Io, s: &str) {
    (io.out)(format!("{s}\n"));
}

pub fn run(c: &Ctx, action: Action, cancel: &wallpick::Cancel, io: Io) -> i32 {
    match try_run(c, action, cancel, io) {
        Ok(()) => 0,
        Err(e) => {
            (io.err)(format!("{e}\n"));
            1
        }
    }
}

fn try_run(c: &Ctx, action: Action, cancel: &wallpick::Cancel, io: Io) -> Result<(), String> {
    let dirs = c.dirs();
    let e = |e: wallpick::Error| e.to_string();
    match action {
        Action::Pick { by } => pick(c, &dirs, by, cancel, io),
        Action::Revert => revert(c, &dirs, io),
        Action::Keep => {
            before::forget(&dirs.state);
            Ok(())
        }
        Action::Like => {
            wallpick::vote(&dirs, wallpick::Vote::Like).map_err(e)?;
            say(io, "Noted: more like this");
            Ok(())
        }
        Action::Dislike { next } => {
            wallpick::vote(&dirs, wallpick::Vote::Dislike).map_err(e)?;
            if next {
                return pick(c, &dirs, By::Same, cancel, io);
            }
            say(io, "Noted: less like this");
            Ok(())
        }
        Action::Style => style(c, &dirs, None, io),
        Action::Arrange { parts } => {
            say(io, "Arranging");
            arrange(c, &parts)?;
            say(io, "Arranged");
            Ok(())
        }
        Action::ResetTaste => {
            wallpick::reset_taste(&dirs).map_err(e)?;
            say(io, "Likes and dislikes forgotten");
            Ok(())
        }
    }
}

fn prefs(c: &Ctx) -> Result<wallpick::Prefs, String> {
    let v = c.config()?;
    let section = v
        .pointer("/background/wallpick")
        .cloned()
        .unwrap_or_else(|| json!({}));
    serde_json::from_value(section).map_err(|e| format!("Wallpaper picker settings: {e}"))
}

/// The user's words as the image model reads them: English. Words in another script go
/// through the translator of the sidebar.
fn english(words: &str, io: Io) -> Result<String, String> {
    if words.is_ascii() {
        return Ok(words.to_string());
    }
    say(io, "Translating the request");
    crate::jobs_wall_gcloud_trans::trans::to_english(words)
        .map_err(|e| format!("Could not translate the request, write it in English ({e})"))
}

fn pick(
    c: &Ctx,
    dirs: &wallpick::Dirs,
    by: By,
    cancel: &wallpick::Cancel,
    io: Io,
) -> Result<(), String> {
    let prefs = prefs(c)?;
    // The picture on the desktop: of a video wallpaper, its still (Background.qml).
    let cfg = c.config()?;
    let at = |p: &str| cfg.pointer(p).and_then(J::as_str).unwrap_or_default();
    let video = ["mp4", "webm", "mkv", "avi", "mov"];
    let wall = match Path::new(at("/background/wallpaperPath")).extension() {
        Some(e) if video.iter().any(|v| e.eq_ignore_ascii_case(v)) => {
            at("/background/thumbnailPath")
        }
        _ => at("/background/wallpaperPath"),
    }
    .to_string();
    let words;
    let by = match &by {
        By::Prefs => wallpick::By::Prefs,
        By::Same => wallpick::By::Same,
        By::Desktop if Path::new(&wall).is_file() => wallpick::By::Desktop(Path::new(&wall)),
        By::Desktop => return Err("The desktop has no wallpaper picture to go by".into()),
        By::Words(w) => {
            words = english(w, io)?;
            wallpick::By::Words(&words)
        }
    };
    let progress = |s: wallpick::Stage| {
        let line = match s {
            wallpick::Stage::Model => "Loading the image model".to_string(),
            wallpick::Stage::Candidates => "Looking for candidates".to_string(),
            wallpick::Stage::Scoring { done, total } => format!("Comparing {done} of {total}"),
            wallpick::Stage::Download => "Downloading".to_string(),
            wallpick::Stage::Accent => "Choosing the accent".to_string(),
        };
        say(io, &line);
    };
    let picked =
        wallpick::pick_by(by, &prefs, dirs, &progress, cancel).map_err(|e| e.to_string())?;
    match by {
        wallpick::By::Prefs => before::forget(&dirs.state),
        _ => before::remember(c, &dirs.state)?,
    }
    say(io, "Applying");
    apply(c, &picked.path, picked.accent.as_deref(), io)?;
    if !c
        .config()?
        .pointer("/background/wallpick/layoutWish")
        .and_then(J::as_str)
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        style(c, dirs, Some(&picked.path), io)?;
    }
    say(io, &picked.page);
    Ok(())
}

/// The look of the desktop before the picks that are still to be kept or put back.
mod before {
    use super::{Ctx, J};
    use serde_json::json;
    use std::path::{Path, PathBuf};

    fn file(state: &Path) -> PathBuf {
        state.join("before.json")
    }

    /// What a pick changes, as it is now; kept as it is when an earlier pick is still not
    /// kept: that one's "before" is the look to go back to.
    pub(super) fn remember(c: &Ctx, state: &Path) -> Result<(), String> {
        let file = file(state);
        if file.exists() {
            return Ok(());
        }
        let cfg = c.config()?;
        let at = |p: &str| cfg.pointer(p).cloned().unwrap_or(J::Null);
        let saved = json!({
            "wallpaperPath": at("/background/wallpaperPath"),
            "accentColor": at("/appearance/palette/accentColor"),
            "barLayout": at("/bar/layout"),
            "clock": {
                "placementStrategy": at("/background/widgets/clock/placementStrategy"),
                "x": at("/background/widgets/clock/x"),
                "y": at("/background/widgets/clock/y"),
            },
            "layout": std::fs::read_to_string(c.layout_file()).ok(),
        });
        let e = |e: std::io::Error| format!("Could not save {}: {e}", file.display());
        std::fs::create_dir_all(state).map_err(e)?;
        std::fs::write(&file, saved.to_string()).map_err(e)
    }

    pub(super) fn forget(state: &Path) {
        let _ = std::fs::remove_file(file(state));
    }

    /// The remembered look, taken: `None` when no pick is waiting to be kept.
    pub(super) fn take(state: &Path) -> Option<J> {
        let v = serde_json::from_str(&std::fs::read_to_string(file(state)).ok()?).ok();
        forget(state);
        v
    }
}

/// Puts back what [`before::remember`] saved: the bar's groups, the desktop clock's place,
/// the layout file with what was arranged since, then the wallpaper and its colors.
fn revert(c: &Ctx, dirs: &wallpick::Dirs, io: Io) -> Result<(), String> {
    let Some(was) = before::take(&dirs.state) else {
        return Err("There is no earlier style to put back".into());
    };
    say(io, "Putting the previous style back");
    let file = c.config_file();
    for side in ["left", "center", "right"] {
        if let Some(groups) = was["barLayout"].get(side).filter(|g| g.is_array()) {
            crate::shell_config::set_json(&file, &["bar", "layout", side], groups.clone());
        }
    }
    for key in ["placementStrategy", "x", "y"] {
        if let Some(v) = was["clock"].get(key).filter(|v| !v.is_null()) {
            let path = ["background", "widgets", "clock", key];
            crate::shell_config::set_json(&file, &path, v.clone());
        }
    }
    if let Some(text) = was["layout"].as_str() {
        let e = |e: std::io::Error| format!("Could not put the layout back: {e}");
        let old = dirs.state.join("before-layout.toml");
        std::fs::write(&old, text).map_err(e)?;
        let (_, loaded) = crate::layout_model::Store::open(&old);
        let _ = std::fs::remove_file(&old);
        crate::layout_model::arrange::in_file(&c.layout_file(), |l| {
            let changed = *l != loaded.layout;
            *l = loaded.layout;
            changed
        })
        .map_err(e)?;
    }
    let accent = was["accentColor"].as_str().filter(|a| !a.is_empty());
    match was["wallpaperPath"].as_str().unwrap_or_default() {
        // No wallpaper was chosen before: the shell shows its own again.
        "" => {
            crate::shell_config::set(&file, &["background", "wallpaperPath"], "");
            let hex = accent.unwrap_or_default();
            crate::shell_config::set(&file, &["appearance", "palette", "accentColor"], hex);
            own_colors(c, accent, io)?;
        }
        wall if Path::new(wall).is_file() => apply(c, Path::new(wall), accent, io)?,
        _ => return Err("The previous wallpaper is no longer there".into()),
    }
    say(io, "The previous style is back");
    Ok(())
}

/// The colors of a desktop with no wallpaper chosen: the accent's, or those of the shell's own
/// wallpaper. The run is told all of it: config.json still has the picked wallpaper and
/// accent until the shell's Config saves, and the scheme in use is the picked wallpaper's.
fn own_colors(c: &Ctx, accent: Option<&str>, io: Io) -> Result<(), String> {
    use colorgen::switchwall::{Accent, Request};
    let p = paths(c);
    // The shell's tree (`colors_dir` is its `scripts/colors`), then where an install by
    // hand keeps it, as switchwall itself looks.
    let own = "assets/images/default_wallpaper.png";
    let own = [
        c.colors_dir.join("../..").join(own),
        p.qs_config_dir().join(own),
    ]
    .into_iter()
    .find(|f| f.is_file());
    if own.is_none() && accent.is_none() {
        return Ok(());
    }
    let config: J = std::fs::read(c.config_file())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let scheme = config["appearance"]["palette"]["type"].as_str();
    let req = Request {
        image: own.map(|f| f.display().to_string()).unwrap_or_default(),
        scheme: scheme.filter(|s| !s.is_empty()).unwrap_or("auto").into(),
        keep_wallpaper: true,
        accent: Some(accent.map_or(Accent::Clear, |hex| Accent::Set(hex.into()))),
        ..Default::default()
    };
    match colorgen::switchwall::run(&req, &p, &c.sys, io) {
        0 => Ok(()),
        code => Err(format!(
            "Could not make the previous colors again (exit {code})"
        )),
    }
}

fn paths(c: &Ctx) -> colorgen::switchwall::Paths {
    colorgen::switchwall::Paths {
        config_home: c.config_home.clone(),
        cache_home: c.cache_home.clone(),
        state_home: c.state_home.clone(),
        script_dir: c.colors_dir.clone(),
        home: c.home.clone(),
    }
}

/// The wallpaper and its colors, like `switchwall.sh --image <path>`; with an accent the
/// wallpaper is set first and the colors are generated from that accent instead.
pub fn apply(c: &Ctx, path: &Path, accent: Option<&str>, io: Io) -> Result<(), String> {
    colorgen::switchwall::CATEGORIZE.get_or_init(|| crate::jobs_ai::categorize::for_switchwall);
    let p = paths(c);
    let path = path.display().to_string();
    let req = match accent {
        Some(hex) => {
            c.sys
                .set_config(&c.config_file(), &["background", "wallpaperPath"], &path);
            colorgen::switchwall::Request {
                image: path,
                keep_wallpaper: true,
                accent: Some(colorgen::switchwall::Accent::Set(hex.into())),
                ..Default::default()
            }
        }
        None => colorgen::switchwall::Request {
            image: path,
            ..Default::default()
        },
    };
    match colorgen::switchwall::run(&req, &p, &c.sys, io) {
        0 => Ok(()),
        code => Err(format!(
            "Could not apply the wallpaper (switchwall exited {code})"
        )),
    }
}

/// The bar arrangement from the layout wish and a calm spot for the desktop clock.
fn style(c: &Ctx, dirs: &wallpick::Dirs, wall: Option<&Path>, io: Io) -> Result<(), String> {
    let cfg = c.config()?;
    let file = c.config_file();
    let wish = cfg
        .pointer("/background/wallpick/layoutWish")
        .and_then(J::as_str)
        .unwrap_or_default();
    say(io, "Arranging");
    if let Some(l) = wallpick::bar_layout(wish, dirs).map_err(|e| e.to_string())? {
        let names = |groups: &[wallpick::BarGroup]| -> Vec<String> {
            groups.iter().map(|g| g.name().to_string()).collect()
        };
        arrange(c, &[names(&l.left), names(&l.center), names(&l.right)])?;
    }
    // A wallpaper applied in this run is given: the shell's Config saves it to the file only
    // after the run, which still names the one before.
    let wall = wall.map(Path::to_path_buf).unwrap_or_else(|| {
        let at = cfg.pointer("/background/wallpaperPath");
        at.and_then(J::as_str).unwrap_or_default().into()
    });
    if wall.is_file() {
        let (x, y) =
            wallpick::clock_spot(&wall, c.screen, CLOCK_SIZE, dirs).map_err(|e| e.to_string())?;
        let clock = ["background", "widgets", "clock"];
        let at = |k: &'static str| [clock[0], clock[1], clock[2], k];
        crate::shell_config::set(&file, &at("placementStrategy"), "free");
        crate::shell_config::set_json(&file, &at("x"), json!(x.round()));
        crate::shell_config::set_json(&file, &at("y"), json!(y.round()));
        let (x, y) = (x.round() as i32, y.round() as i32);
        crate::layout_model::arrange::in_file(&c.layout_file(), |l| {
            crate::layout_model::arrange::desktop_clock(l, x, y)
        })
        .map_err(|e| format!("Could not save the layout: {e}"))?;
    }
    say(io, "Arranged");
    Ok(())
}

/// The original's bar groups to `bar.layout` (the bar the original draws without a layout) and to the layout's
/// panel at the original's bar edge, the bar the shell draws.
fn arrange(c: &Ctx, parts: &[Vec<String>; 3]) -> Result<(), String> {
    let file = c.config_file();
    for (side, groups) in ["left", "center", "right"].into_iter().zip(parts) {
        crate::shell_config::set_json(&file, &["bar", "layout", side], json!(groups));
    }
    crate::layout_model::arrange::in_file(&c.layout_file(), |l| {
        crate::layout_model::arrange::bar_groups(l, [&parts[0], &parts[1], &parts[2]])
    })
    .map_err(|e| format!("Could not save the layout: {e}"))?;
    Ok(())
}

/// The cookie clock's footprint with its date bubble (`ClockWidget`'s implicit size).
const CLOCK_SIZE: (u32, u32) = (230, 230);

#[cfg(test)]
mod tests {
    use super::{Action, By};

    fn parse(a: &[&str]) -> Option<Action> {
        Action::parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    use super::{Ctx, run};
    use colorgen::switchwall::Io;
    use serde_json::{Value, json};
    use std::path::Path;
    use std::sync::Mutex;

    fn ctx(home: &std::path::Path, config: Value) -> Ctx {
        let file = home.join(".config/impulse-shell/config.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, config.to_string()).unwrap();
        Ctx {
            home: home.into(),
            config_home: home.join(".config"),
            state_home: home.join(".local/state"),
            cache_home: home.join(".cache"),
            pictures: home.join("Pictures"),
            colors_dir: home.join("scripts/colors"),
            sys: crate::shell_config::sys(),
            screen: (1920, 1080),
        }
    }

    fn exec(c: &Ctx, a: Action) -> (i32, String, String) {
        let (out, err) = (Mutex::new(String::new()), Mutex::new(String::new()));
        let o = |s: String| out.lock().unwrap().push_str(&s);
        let e = |s: String| err.lock().unwrap().push_str(&s);
        let code = run(c, a, &wallpick::Cancel::new(), Io { out: &o, err: &e });
        (code, out.into_inner().unwrap(), err.into_inner().unwrap())
    }

    #[test]
    fn a_broken_config_is_reported_instead_of_picking_with_the_defaults() {
        let t = tempfile::tempdir().unwrap();
        let c = ctx(t.path(), json!({}));
        std::fs::write(c.config_file(), "{\"background\": {}}}").unwrap();
        let (code, _, err) = exec(&c, Action::Pick { by: By::Prefs });
        assert_ne!(code, 0);
        assert!(err.contains("config.json"), "{err}");
    }

    fn config(c: &Ctx) -> Value {
        serde_json::from_str(&std::fs::read_to_string(c.config_file()).unwrap()).unwrap()
    }

    #[test]
    fn style_writes_the_wished_bar_layout_as_lists() {
        let t = tempfile::tempdir().unwrap();
        let wish =
            json!({"background": {"wallpick": {"layoutWish": "часы слева, музыка по центру"}}});
        let c = ctx(t.path(), wish);
        let (code, out, err) = exec(&c, Action::Style);
        assert_eq!(code, 0, "{err}");
        assert!(out.lines().any(|l| l == "Arranged"), "{out}");
        let v = config(&c);
        let left = v
            .pointer("/bar/layout/left")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(left.first(), Some(&json!("clock")));
        assert_eq!(v.pointer("/bar/layout/center"), Some(&json!(["media"])));
        // No wallpaper yet: the desktop clock keeps its placement
        assert_eq!(v.pointer("/background/widgets/clock"), None);
    }

    #[test]
    fn arrange_moves_the_bar_groups_in_the_config_and_the_layout() {
        use crate::layout_model::rusty::{Monitor, from_options};
        let t = tempfile::tempdir().unwrap();
        let c = ctx(t.path(), json!({}));
        let a = parse(&["arrange", "media", "workspaces", "clock"]).unwrap();
        assert_eq!(
            exec(&c, a.clone()).0,
            0,
            "without a layout only the config changes"
        );
        assert!(!c.layout_file().exists());
        let monitor = Monitor {
            connector: "DP-1".into(),
        };
        std::fs::create_dir_all(c.layout_file().parent().unwrap()).unwrap();
        std::fs::write(
            c.layout_file(),
            from_options(&json!({}), &monitor).to_toml(),
        )
        .unwrap();
        let (code, out, err) = exec(&c, a);
        assert_eq!(code, 0, "{err}");
        assert!(out.lines().any(|l| l == "Arranged"), "{out}");
        assert_eq!(
            config(&c).pointer("/bar/layout/left"),
            Some(&json!(["media"]))
        );
        let layout = std::fs::read_to_string(c.layout_file()).unwrap();
        let clock = layout.find("id = \"clock\"").unwrap();
        assert!(
            layout[clock..]
                .lines()
                .take(6)
                .any(|l| l.contains("zone = \"end\"")),
            "{layout}"
        );
    }

    #[test]
    fn revert_puts_back_what_was_there_before_the_first_pick_not_yet_kept() {
        let t = tempfile::tempdir().unwrap();
        let before = json!({
            "bar": {"layout": {"left": ["workspaces"], "center": ["clock"], "right": ["media"]}},
            "background": {"widgets": {"clock": {"placementStrategy": "leastBusy", "x": 10, "y": 20}}},
        });
        let c = ctx(t.path(), before.clone());
        let state = c.dirs().state;
        super::before::remember(&c, &state).unwrap();
        // What a pick with a layout wish changes; a second pick's "before" is not taken.
        let after = json!({
            "bar": {"layout": {"left": ["media"], "center": [], "right": ["clock", "workspaces"]}},
            "background": {"widgets": {"clock": {"placementStrategy": "free", "x": 700, "y": 300}}},
        });
        std::fs::write(c.config_file(), after.to_string()).unwrap();
        super::before::remember(&c, &state).unwrap();

        // No wallpaper was chosen before: none is chosen again, and the rest is back.
        let (code, out, err) = exec(&c, Action::Revert);
        assert_eq!(code, 0, "{out}{err}");
        assert!(out.contains("The previous style is back"), "{out}");
        let now = config(&c);
        assert_eq!(now["bar"], before["bar"]);
        assert_eq!(
            now["background"]["widgets"],
            before["background"]["widgets"]
        );
        assert_eq!(now["background"]["wallpaperPath"], "");

        let (code, _, err) = exec(&c, Action::Revert);
        assert_eq!(code, 1);
        assert!(err.contains("no earlier style"), "{err}");

        // A wallpaper that was deleted since cannot come back; the run says so.
        std::fs::write(
            c.config_file(),
            json!({"background": {"wallpaperPath": "/gone.png"}}).to_string(),
        )
        .unwrap();
        super::before::remember(&c, &state).unwrap();
        let (code, _, err) = exec(&c, Action::Revert);
        assert_eq!(code, 1);
        assert!(err.contains("no longer there"), "{err}");
    }

    /// Esc after a pick on a desktop that had no wallpaper chosen: the colors are again those
    /// of the shell's own wallpaper, not the picked wallpaper's that the files still name.
    #[test]
    fn revert_to_no_wallpaper_makes_the_colors_of_the_shells_own_again() {
        let t = tempfile::tempdir().unwrap();
        let mut c = ctx(t.path(), json!({}));
        let pics = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../colorgen/tests/switchwall/fixture/home/Pictures/Wallpapers"
        );
        // A shell tree: the terminal templates the colors need, and its own wallpaper.
        let shell = t.path().join("shell");
        c.colors_dir = shell.join("scripts/colors");
        let (terminal, images) = (c.colors_dir.join("terminal"), shell.join("assets/images"));
        std::fs::create_dir_all(&terminal).unwrap();
        std::fs::create_dir_all(&images).unwrap();
        let shipped = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../qml/scripts/colors/terminal"
        );
        for f in std::fs::read_dir(shipped).unwrap() {
            let f = f.unwrap();
            std::fs::copy(f.path(), terminal.join(f.file_name())).unwrap();
        }
        let own = images.join("default_wallpaper.png");
        std::fs::copy(format!("{pics}/syn_gray.png"), own).unwrap();
        let generated = c.state_home.join("impulse-shell/user/generated");
        let colors = || std::fs::read_to_string(generated.join("material_colors.scss")).unwrap();
        let io = Io {
            out: &|_| {},
            err: &|_| {},
        };
        super::own_colors(&c, None, io).unwrap();
        let was = colors();
        assert!(was.contains("$primary"), "{was}");

        let state = c.dirs().state;
        super::before::remember(&c, &state).unwrap();
        let picked = format!("{pics}/syn_grad.jpg");
        super::apply(&c, Path::new(&picked), Some("#FDB975"), io).unwrap();
        assert_ne!(colors(), was);
        assert_eq!(config(&c)["background"]["wallpaperPath"], picked);

        let (code, out, err) = exec(&c, Action::Revert);
        assert_eq!(code, 0, "{out}{err}");
        assert_eq!(colors(), was);
    }

    #[test]
    fn keep_and_an_unreviewed_pick_leave_nothing_to_put_back() {
        let t = tempfile::tempdir().unwrap();
        let c = ctx(t.path(), json!({}));
        let state = c.dirs().state;
        super::before::remember(&c, &state).unwrap();
        assert_eq!(exec(&c, Action::Keep).0, 0);
        assert!(super::before::take(&state).is_none());
    }

    #[test]
    fn a_pick_by_the_desktop_needs_a_wallpaper_picture() {
        let t = tempfile::tempdir().unwrap();
        let c = ctx(
            t.path(),
            json!({"background": {"wallpaperPath": "/nowhere.png"}}),
        );
        let (code, _, err) = exec(&c, Action::Pick { by: By::Desktop });
        assert_eq!(code, 1);
        assert!(err.contains("no wallpaper picture"), "{err}");
    }

    #[test]
    fn like_without_a_pick_fails_with_a_message() {
        let t = tempfile::tempdir().unwrap();
        let c = ctx(t.path(), json!({}));
        let (code, _, err) = exec(&c, Action::Like);
        assert_eq!(code, 1);
        assert!(!err.trim().is_empty());
        assert!(config(&c).pointer("/bar").is_none());
    }

    #[test]
    fn reset_taste_succeeds_on_a_fresh_state() {
        let t = tempfile::tempdir().unwrap();
        let c = ctx(t.path(), json!({}));
        assert_eq!(exec(&c, Action::ResetTaste).0, 0);
    }

    #[test]
    fn actions() {
        assert_eq!(parse(&["pick"]), Some(Action::Pick { by: By::Prefs }));
        assert_eq!(
            parse(&["pick", "--desktop"]),
            Some(Action::Pick { by: By::Desktop })
        );
        assert_eq!(
            parse(&["pick", "--words", " ночной город "]),
            Some(Action::Pick {
                by: By::Words("ночной город".into())
            })
        );
        assert_eq!(parse(&["pick", "--words", " "]), None);
        assert_eq!(parse(&["pick", "--words"]), None);
        assert_eq!(parse(&["revert"]), Some(Action::Revert));
        assert_eq!(parse(&["keep"]), Some(Action::Keep));
        assert_eq!(
            parse(&["arrange", "", "workspaces,clock", "media"]),
            Some(Action::Arrange {
                parts: [
                    vec![],
                    vec!["workspaces".into(), "clock".into()],
                    vec!["media".into()]
                ]
            })
        );
        assert_eq!(parse(&["like"]), Some(Action::Like));
        assert_eq!(parse(&["dislike"]), Some(Action::Dislike { next: false }));
        assert_eq!(
            parse(&["dislike", "--next"]),
            Some(Action::Dislike { next: true })
        );
        assert_eq!(parse(&["style"]), Some(Action::Style));
        assert_eq!(parse(&["reset-taste"]), Some(Action::ResetTaste));
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&["pick", "extra"]), None);
        assert_eq!(parse(&["like", "--next"]), None);
    }
}
