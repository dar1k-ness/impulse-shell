//! The wallpaper picker's actions (services/Wallpick.qml's `WallpickRun`, and the
//! `rusty-impulse wallpick <action>` command), run in-process: the engine is the
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
        self.config_home.join("rusty-impulse/config.json")
    }

    /// The shell's layout (`layout_model::default_path` under this config home).
    fn layout_file(&self) -> PathBuf {
        self.config_home.join("rusty-impulse/layout.toml")
    }

    fn dirs(&self) -> wallpick::Dirs {
        wallpick::Dirs {
            state: self.state_home.join("rustshell/user/wallpick"),
            cache: self.cache_home.join("rustshell/wallpick"),
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

/// `rusty-impulse wallpick <action>` outside a running shell (scripts, keybinds).
pub fn cli(args: &[String], root: &Path) -> i32 {
    let Some(action) = Action::parse(args) else {
        elog::eprintln!(
            "usage: rusty-impulse wallpick pick | like | dislike [--next] | style | reset-taste | arrange LEFT CENTER RIGHT"
        );
        return 2;
    };
    let ctx = Ctx::from_env(root, (1920, 1080));
    run(&ctx, action, &wallpick::Cancel::default(), Io::STD)
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Pick,
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

impl Action {
    pub fn parse(args: &[String]) -> Option<Action> {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        Some(match a.as_slice() {
            ["pick"] => Action::Pick,
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
        Action::Pick => pick(c, &dirs, cancel, io),
        Action::Like => {
            wallpick::vote(&dirs, wallpick::Vote::Like).map_err(e)?;
            say(io, "Noted: more like this");
            Ok(())
        }
        Action::Dislike { next } => {
            wallpick::vote(&dirs, wallpick::Vote::Dislike).map_err(e)?;
            if next {
                return pick(c, &dirs, cancel, io);
            }
            say(io, "Noted: less like this");
            Ok(())
        }
        Action::Style => style(c, &dirs, io),
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

fn pick(c: &Ctx, dirs: &wallpick::Dirs, cancel: &wallpick::Cancel, io: Io) -> Result<(), String> {
    let prefs = prefs(c)?;
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
    let picked = wallpick::pick(&prefs, dirs, &progress, cancel).map_err(|e| e.to_string())?;
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
        style(c, dirs, io)?;
    }
    say(io, &picked.page);
    Ok(())
}

/// The wallpaper and its colors, like `switchwall.sh --image <path>`; with an accent the
/// wallpaper is set first and the colors are generated from that accent instead.
pub fn apply(c: &Ctx, path: &Path, accent: Option<&str>, io: Io) -> Result<(), String> {
    colorgen::switchwall::CATEGORIZE.get_or_init(|| crate::jobs_ai::categorize::for_switchwall);
    let p = colorgen::switchwall::Paths {
        config_home: c.config_home.clone(),
        cache_home: c.cache_home.clone(),
        state_home: c.state_home.clone(),
        script_dir: c.colors_dir.clone(),
        home: c.home.clone(),
    };
    let path = path.display().to_string();
    let req = match accent {
        Some(hex) => {
            c.sys
                .set_config(&c.config_file(), &["background", "wallpaperPath"], &path);
            colorgen::switchwall::Request {
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
fn style(c: &Ctx, dirs: &wallpick::Dirs, io: Io) -> Result<(), String> {
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
    let wall = cfg
        .pointer("/background/wallpaperPath")
        .and_then(J::as_str)
        .unwrap_or_default();
    if !wall.is_empty() && Path::new(wall).is_file() {
        let (x, y) = wallpick::clock_spot(Path::new(wall), c.screen, CLOCK_SIZE, dirs)
            .map_err(|e| e.to_string())?;
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
    use super::Action;

    fn parse(a: &[&str]) -> Option<Action> {
        Action::parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    use super::{Ctx, run};
    use colorgen::switchwall::Io;
    use serde_json::{Value, json};
    use std::sync::Mutex;

    fn ctx(home: &std::path::Path, config: Value) -> Ctx {
        let file = home.join(".config/rusty-impulse/config.json");
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
        let (code, _, err) = exec(&c, Action::Pick);
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
        assert_eq!(parse(&["pick"]), Some(Action::Pick));
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
