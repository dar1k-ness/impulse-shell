//! scripts/colors/random/random_konachan_wall.sh and random_osu_wall.sh: pick a wallpaper from
//! the site's API (curl + jq in the scripts), download it to `$PICTURES/Wallpapers` and hand
//! it to `switchwall.sh --image`, the script's last command, here the in-process
//! `colorgen::switchwall` (same output, exit code and serialization as other native runs).
//!
//! Deliberate difference: the scripts hand switchwall.sh whatever they
//! got, so a site that is down or answers with an error page sets a broken path and a black
//! wallpaper. Here any failure (no image link, HTTP error, HTML page, empty body, a server
//! silent for `http::STALL`) leaves no file and the current wallpaper alone, exit code 1.
use crate::jobs::Job;
use crate::runtime::World;
use colorgen::switchwall::{Accent, Io, Request};
use colorgen::sys::Sys;
use serde_json::Value as J;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Site {
    Konachan,
    Osu,
}

/// What the scripts read from their environment, and the endpoints they hard-code.
pub(super) struct Ctx {
    pub home: String,
    pub xdg_config_home: String,
    pub xdg_pictures_dir: Option<String>,
    pub has_xdg_user_dir: bool,
    pub script_dir: String,
    pub konachan: String,
    pub osu: String,
    /// `"$SCRIPT_DIR/../switchwall.sh" --image <path>`: [`native_switchwall`] outside tests.
    pub switchwall: fn(&Ctx, &Request, Io) -> i32,
    pub sys: Arc<dyn Sys>,
    pub xdg_cache_home: String,
    pub xdg_state_home: String,
}

impl Ctx {
    fn new(script_dir: String) -> Ctx {
        Ctx {
            home: super::env("HOME"),
            xdg_config_home: super::env("XDG_CONFIG_HOME"),
            xdg_pictures_dir: std::env::var("XDG_PICTURES_DIR").ok(),
            has_xdg_user_dir: crate::osutil::on_path("xdg-user-dir"),
            script_dir,
            konachan: "https://konachan.net".into(),
            osu: "https://osu.ppy.sh".into(),
            switchwall: native_switchwall,
            sys: crate::shell_config::sys(),
            xdg_cache_home: super::env("XDG_CACHE_HOME"),
            xdg_state_home: super::env("XDG_STATE_HOME"),
        }
    }
}

pub static RANDOM: crate::impulse::Element = crate::impulse::Element {
    job: Some(random),
    ..crate::impulse::Element::new("RandomWallpaper")
};

/// `Impulse/RandomWallpaper`: downloads a random wallpaper from `source` (`"konachan"` or
/// `"osu"`) into the pictures folder and switches to it.
fn random(w: &mut World, p: usize) -> Option<Job> {
    let site = match crate::jobs::text(w, p, "source").as_str() {
        "konachan" => Site::Konachan,
        "osu" => Site::Osu,
        _ => return None,
    };
    let root = w.root_dir.as_ref()?.display().to_string();
    Some(job(site, Ctx::new(format!("{root}/scripts/colors/random"))))
}

/// services/Switchwall.qml's `proc`, a `WallpaperSwitch`: the in-process
/// `colorgen::switchwall` for what its properties ask (`image`, `mode`, `schemeType`, the
/// accent `color` or `clearColor` or `pickColor`, `noSwitch`). Its turn is taken here, when
/// the queue starts the run, so runs started elsewhere meanwhile go after it.
pub(crate) fn switchwall_proc(w: &mut World, p: usize) -> Option<Job> {
    let dir = w.root_dir.as_ref()?.join("scripts/colors");
    let (text, flag) = (crate::jobs::text, crate::jobs::flag);
    let color = text(w, p, "color");
    let request = Request {
        image: text(w, p, "image"),
        mode: text(w, p, "mode"),
        scheme: text(w, p, "schemeType"),
        accent: if colorgen::switchwall::is_hex(&color) {
            Some(Accent::Set(color))
        } else if flag(w, p, "clearColor") {
            Some(Accent::Clear)
        } else if flag(w, p, "pickColor") {
            Some(Accent::Pick)
        } else {
            None
        },
        keep_wallpaper: flag(w, p, "noSwitch"),
    };
    colorgen::switchwall::CATEGORIZE.get_or_init(|| crate::jobs_ai::categorize::for_switchwall);
    let sys = crate::shell_config::sys();
    let turn = colorgen::switchwall::Turn::take();
    Some(Box::new(move |io, _| {
        let paths = colorgen::switchwall::Paths::from_env(dir);
        let (out, err) = (|s| io.out(s), |s| io.err(s));
        colorgen::switchwall::run_in_turn(
            turn,
            &request,
            &paths,
            &sys,
            Io {
                out: &out,
                err: &err,
            },
        )
    }))
}

fn job(site: Site, ctx: Ctx) -> Job {
    Box::new(move |io, _| run(site, &ctx, &|s| io.out(s), &|s| io.err(s)))
}

/// `get_pictures_dir` (xdg-user-dir PICTURES, else the user-dirs.dirs fallback).
pub(super) fn pictures_dir(c: &Ctx) -> String {
    let cfg = if c.xdg_config_home.is_empty() {
        format!("{}/.config", c.home)
    } else {
        c.xdg_config_home.clone()
    };
    let file = std::fs::read(format!("{cfg}/user-dirs.dirs"))
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned());
    let from_file = file.as_deref().map(|f| user_dirs_value(f, &c.home));
    if c.has_xdg_user_dir {
        // `. user-dirs.dirs; eval echo ${XDG_PICTURES_DIR:-$HOME}`
        let v = from_file
            .flatten()
            .or_else(|| c.xdg_pictures_dir.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| c.home.clone());
        return v.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    match from_file {
        // `${pictures_path/#\$HOME/$HOME}`; already expanded by the sourcing above.
        Some(v) => v.or_else(|| c.xdg_pictures_dir.clone()).unwrap_or_default(),
        None => format!("{}/Pictures", c.home),
    }
}

/// The last `XDG_PICTURES_DIR=` assignment of a sourced user-dirs.dirs, `$HOME` expanded.
fn user_dirs_value(file: &str, home: &str) -> Option<String> {
    let mut v = None;
    for l in file.lines() {
        let Some(rhs) = l.trim_start().strip_prefix("XDG_PICTURES_DIR=") else {
            continue;
        };
        let mut o = String::new();
        let mut cs = rhs.chars().peekable();
        let mut q = None;
        while let Some(c) = cs.next() {
            match (q, c) {
                (None, '"' | '\'') => q = Some(c),
                (Some(x), c) if c == x => q = None,
                (None, ' ' | '\t' | '#' | ';') => break,
                (Some('\''), c) => o.push(c),
                (_, '\\') => {
                    if let Some(n) = cs.next() {
                        o.push(n);
                    }
                }
                (_, '$') => {
                    let rest: String = cs.clone().collect();
                    if rest.starts_with("HOME") {
                        o.push_str(home);
                        for _ in 0..4 {
                            cs.next();
                        }
                    } else if rest.starts_with("{HOME}") {
                        o.push_str(home);
                        for _ in 0..6 {
                            cs.next();
                        }
                    } else {
                        o.push('$');
                    }
                }
                (_, c) => o.push(c),
            }
        }
        v = Some(o);
    }
    v
}

fn io_reason(e: &std::io::Error) -> String {
    let s = e.to_string();
    s.split(" (os error").next().unwrap_or(&s).to_string()
}

/// The JSON a GET of `url` answers with; `None` (and the reason on `err`) when the request
/// fails or the body is not JSON.
fn get_json(url: &str, ua: Option<&str>, err: &dyn Fn(String)) -> Option<J> {
    let headers: Vec<(String, String)> = ua
        .map(|u| vec![("User-Agent".to_string(), u.to_string())])
        .unwrap_or_default();
    let text = crate::http::get(url, &headers, STALL).and_then(|r| Ok(r.text()?));
    match text.map(|t| serde_json::from_str(&t)) {
        Ok(Ok(v)) => Some(v),
        Ok(Err(e)) => {
            err(format!("random wallpaper: {url}: {e}\n"));
            None
        }
        Err(e) => {
            err(format!("random wallpaper: {url}: {e}\n"));
            None
        }
    }
}

/// The shell's `config.json`; null when it cannot be read.
fn config(path: &str) -> J {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(J::Null)
}

const STALL: crate::http::Opts = crate::http::Opts {
    follow: false,
    ipv4: false,
    stall: true,
    answer_within: None,
};

/// Most a downloaded wallpaper may hold (the API's server decides what it sends).
const WALL_MAX: u64 = 128 << 20;

/// Saves the image at `link` as `path`. Only a real image reaches `path`: an HTTP error, an
/// HTML/text page, an empty body or one over [`WALL_MAX`] is an error and writes nothing.
fn download(link: &str, ua: Option<&str>, path: &str) -> Result<(), String> {
    if link.is_empty() || link.contains(|c: char| c == ' ' || c.is_control()) {
        return Err(format!("{link:?}: not a URL"));
    }
    let url = if link.contains("://") {
        link.to_string()
    } else {
        format!("http://{link}")
    };
    let headers: Vec<(String, String)> = ua
        .map(|u| vec![("User-Agent".to_string(), u.to_string())])
        .unwrap_or_default();
    let url = crate::http::encode_path(&url);
    let mut r = crate::http::get(&url, &headers, STALL).map_err(|e| format!("{url}: {e}"))?;
    if r.status >= 400 {
        return Err(format!("{url}: HTTP {}", r.status));
    }
    if let Some(t) = r.header("content-type")
        && t.trim_start().to_ascii_lowercase().starts_with("text/")
    {
        return Err(format!("{url}: got {t}, not an image"));
    }
    crate::http::check_length(&r, WALL_MAX).map_err(|e| format!("{url}: {e}"))?;
    let tmp = crate::http::part_path(std::path::Path::new(path));
    let res = std::fs::File::create(&tmp)
        .and_then(|mut f| crate::osutil::copy_max(&mut r.body, &mut f, WALL_MAX))
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::FileTooLarge => format!("{url}: body {e}"),
            _ => format!("{path}: {}", io_reason(&e)),
        })
        .and_then(|n| {
            if n == 0 {
                Err(format!("{url}: empty body"))
            } else {
                std::fs::rename(&tmp, path).map_err(|e| format!("{path}: {}", io_reason(&e)))
            }
        });
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// A uniformly random number below `n` (which is not 0).
fn random_below(n: u64) -> u64 {
    let mut b = [0u8; 8];
    // SAFETY: getrandom(2) writes at most 8 bytes to `b`, a live 8-byte buffer.
    unsafe { libc::getrandom(b.as_mut_ptr().cast(), b.len(), 0) };
    u64::from_ne_bytes(b) % n
}

/// The wallpaper's extension: that of the link's last path segment when it is 1-5 ASCII
/// letters or digits, else `jpg`. The script's `awk -F. '{print $NF}'` took whatever followed
/// the link's last `.`, so the server chose the file name (`https://x.com/a` saved
/// `random_wallpaper.com/a`, a query string became part of it).
fn ext_of(link: &str) -> String {
    let path = link.split(['?', '#']).next().unwrap_or("");
    let name = path.rsplit('/').next().unwrap_or("");
    match name.rsplit_once('.') {
        Some((_, e))
            if (1..=5).contains(&e.len()) && e.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            e.to_string()
        }
        _ => "jpg".to_string(),
    }
}

/// Asks `site` for an image, saves it under the pictures folder's `Wallpapers` and hands it to
/// the wallpaper switch, whose output goes to `out`/`err` and whose code is returned; 1, with
/// the reason on `err` and the wallpaper left alone, when no image could be fetched.
pub(super) fn run(
    site: Site,
    c: &Ctx,
    out: &(dyn Fn(String) + Sync),
    err: &(dyn Fn(String) + Sync),
) -> i32 {
    let pics = pictures_dir(c);
    let walls = format!("{pics}/Wallpapers");
    if let Err(e) = std::fs::create_dir_all(&walls) {
        err(format!("random wallpaper: {walls}: {}\n", io_reason(&e)));
    }
    let cfg = config(&format!("{}/.config/rusty-impulse/config.json", c.home));
    let (link, ua) = match site {
        Site::Konachan => {
            let page = 1 + random_below(1000);
            let ua = match &cfg["networking"]["userAgent"] {
                J::Null | J::Bool(false) => String::new(),
                J::String(s) => s.clone(),
                v => v.to_string(),
            };
            let url = format!(
                "{}/post.json?tags=rating%3Asafe&limit=1&page={page}",
                c.konachan
            );
            let link = get_json(&url, Some(&ua), err)
                .and_then(|r| r[0]["file_url"].as_str().map(String::from));
            (link, Some(ua))
        }
        Site::Osu => {
            let url = format!("{}/api/v2/seasonal-backgrounds", c.osu);
            let link = get_json(&url, None, err).and_then(|r| {
                let all = r["backgrounds"].as_array()?;
                let one = all.get(random_below(all.len().max(1) as u64) as usize)?;
                one["url"].as_str().map(String::from)
            });
            (link, None)
        }
    };
    let Some(link) = link else {
        err("random wallpaper: no image link from the API; wallpaper unchanged\n".into());
        return 1;
    };
    let ext = ext_of(&link);
    let mut path = format!("{walls}/random_wallpaper.{ext}");
    if cfg["background"]["wallpaperPath"].as_str() == Some(&path) {
        path = format!("{walls}/random_wallpaper-1.{ext}");
    }
    if let Err(e) = download(&link, ua.as_deref(), &path) {
        err(format!(
            "{e}\nrandom wallpaper: download failed; wallpaper unchanged\n"
        ));
        return 1;
    }
    let switch = Request {
        image: path,
        ..Default::default()
    };
    (c.switchwall)(c, &switch, Io { out, err })
}

/// Switchwall.qml's `picker`, switchwall.sh's own prompt: kdialog in the pictures folder the
/// script `cd`s to, with its stdout and exit code (1 when no such folder exists, as `cd` fails).
/// Stopping the Process closes the dialog.
pub(crate) fn switchwall_picker() -> Job {
    let home = std::env::var_os("HOME").unwrap_or_default();
    let pics = colorgen::switchwall::pictures_dir(std::path::Path::new(&home));
    picker(pics, colorgen::switchwall::PICK_ARGV[0].into())
}

/// [`switchwall_picker`] with `$(xdg-user-dir PICTURES)` and the dialog's program given.
pub(crate) fn picker(pics: std::path::PathBuf, kdialog: std::path::PathBuf) -> Job {
    Box::new(move |io, stop| {
        use std::os::unix::process::ExitStatusExt;
        let Some(dir) = colorgen::switchwall::pick_dir(pics) else {
            return 1;
        };
        let [cmd, args @ ..] = colorgen::switchwall::PICK_ARGV;
        let ch = crate::child::spawn(
            workpool::userenv::command(&kdialog)
                .args(args)
                .current_dir(dir)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped()),
        );
        let ch = match ch {
            Ok(ch) => ch,
            Err(e) => {
                io.err(format!("{cmd}: {e}\n"));
                return 127;
            }
        };
        let _term = crate::osutil::signal_on_stop(ch.child(), stop, libc::SIGTERM);
        match ch.wait_with_output() {
            Ok((status, out)) => {
                io.out(String::from_utf8_lossy(&out));
                status
                    .code()
                    .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
            }
            Err(e) => {
                io.err(format!("{cmd}: {e}\n"));
                1
            }
        }
    })
}

/// switchwall.sh in-process, with the environment the script would see (`${XDG_*:-default}`).
pub(super) fn native_switchwall(c: &Ctx, req: &Request, io: Io) -> i32 {
    colorgen::switchwall::CATEGORIZE.get_or_init(|| crate::jobs_ai::categorize::for_switchwall);
    let home = std::path::PathBuf::from(&c.home);
    let xdg = |v: &str, d: &str| match v {
        "" => home.join(d),
        v => v.into(),
    };
    let p = colorgen::switchwall::Paths {
        config_home: xdg(&c.xdg_config_home, ".config"),
        cache_home: xdg(&c.xdg_cache_home, ".cache"),
        state_home: xdg(&c.xdg_state_home, ".local/state"),
        // switchwall.sh's own `SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"`.
        script_dir: std::path::Path::new(&c.script_dir)
            .parent()
            .unwrap_or(std::path::Path::new(""))
            .into(),
        home: home.clone(),
    };
    colorgen::switchwall::run(req, &p, &c.sys, io)
}

/// The first argument of the shell binary run in switchwall.sh's place: `rusty-impulse
/// __ri-switchwall DIR [ARGS]` is `DIR/switchwall.sh ARGS` ([`switchwall_serve`]). The tree's
/// `scripts/colors/switchwall.sh`, the file the original's Hyprland keybind names, runs it.
pub const SWITCHWALL_ARG: &str = "__ri-switchwall";

/// switchwall.sh as a program, for a caller outside the shell (the keybind that runs it when no
/// shell answers): `args` are the script's directory, then the script's own. No arguments asks
/// for the image with kdialog. Returns the script's exit status, once the jobs the script
/// leaves behind it have ended; the programs it starts (mpvpaper) stay.
pub fn switchwall_serve(args: &[String]) -> i32 {
    let Some((dir, args)) = args.split_first() else {
        return 2;
    };
    colorgen::switchwall::CATEGORIZE.get_or_init(|| crate::jobs_ai::categorize::for_switchwall);
    let paths = colorgen::switchwall::Paths::from_env(dir.into());
    let code = colorgen::switchwall::main(args, &paths, &crate::shell_config::sys());
    colorgen::switchwall::wait_background();
    code
}

#[cfg(test)]
mod tests;
