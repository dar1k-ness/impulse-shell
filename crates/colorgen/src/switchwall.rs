//! `colors/switchwall.sh`: pick the wallpaper or seed color, run the matugen templates and the
//! terminal palette in-process, then `applycolor.sh` and the post-processing hooks.

use crate::apply;
use crate::fsutil::{first_line, write_atomic};
use crate::sys::Sys;
use material_colors::color::Argb;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

pub const ALLOWED_TYPES: [&str; 9] = [
    "scheme-content",
    "scheme-expressive",
    "scheme-fidelity",
    "scheme-fruit-salad",
    "scheme-monochrome",
    "scheme-neutral",
    "scheme-rainbow",
    "scheme-tonal-spot",
    "auto",
];

pub const VIDEO_OPTS: &str = "no-audio loop hwdec=auto scale=bilinear interpolation=no video-sync=display-resample panscan=1.0 video-scale-x=1.0 video-scale-y=1.0 video-align-x=0.5 video-align-y=0.5 load-scripts=no";

const IFACE_SCHEME: &str = "color-scheme";
const IFACE_GTK: &str = "gtk-theme";

/// The script's directory layout (`${XDG_*:-default}`).
#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
    pub config_home: PathBuf,
    pub cache_home: PathBuf,
    pub state_home: PathBuf,
    /// `$SCRIPT_DIR`: `<config>/scripts/colors`.
    pub script_dir: PathBuf,
}

impl Paths {
    pub fn from_env(script_dir: PathBuf) -> Paths {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let xdg = |k: &str, d: &str| match std::env::var(k) {
            Ok(v) if !v.is_empty() => PathBuf::from(v),
            _ => home.join(d),
        };
        Paths {
            config_home: xdg("XDG_CONFIG_HOME", ".config"),
            cache_home: xdg("XDG_CACHE_HOME", ".cache"),
            state_home: xdg("XDG_STATE_HOME", ".local/state"),
            home,
            script_dir,
        }
    }
    pub fn qs_config_dir(&self) -> PathBuf {
        self.config_home.join("rustshell/impulse-shell")
    }
    pub fn state_dir(&self) -> PathBuf {
        self.state_home.join("rustshell")
    }
    pub fn generated(&self) -> PathBuf {
        self.state_dir().join("user/generated")
    }
    pub fn shell_config(&self) -> PathBuf {
        self.config_home.join("impulse-shell/config.json")
    }
    fn restore_script(&self) -> PathBuf {
        self.config_home
            .join("hypr/custom/scripts/__restore_video_wallpaper.sh")
    }
    fn thumbnail_dir(&self) -> PathBuf {
        self.config_home
            .join("hypr/custom/scripts/mpvpaper_thumbnails")
    }
}

/// `jq -r '.a.b.c' <<< text`: `None` where jq fails (invalid JSON).
pub fn jq_r(text: &str, path: &[&str]) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    let mut cur = &v;
    for k in path {
        cur = match cur {
            Value::Object(o) => o.get(*k).unwrap_or(&Value::Null),
            Value::Null => &Value::Null,
            _ => return None,
        };
    }
    Some(match cur {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    })
}

/// `jq --arg v "$v" '.a.b = $v' file > file.tmp && mv file.tmp file`, as one read-modify-write
/// under an exclusive lock on the file's directory, so concurrent edits never lose each other.
pub fn jq_set(file: &Path, path: &[&str], value: &str) {
    jq_set_value(file, path, Value::String(value.to_string()));
}

/// [`jq_set`] with any JSON value (`jq --argjson v`).
pub fn jq_set_value(file: &Path, path: &[&str], value: Value) {
    let _lock = file
        .parent()
        .and_then(|d| std::fs::File::open(d).ok())
        .inspect(|d| {
            use std::os::fd::AsRawFd;
            // SAFETY: flock(2) on an fd `d` keeps open for the call.
            unsafe { libc::flock(d.as_raw_fd(), libc::LOCK_EX) };
        });
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    let Ok(mut v) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let mut cur = &mut v;
    for (i, k) in path.iter().enumerate() {
        if cur.is_null() {
            *cur = Value::Object(Default::default());
        }
        let Value::Object(o) = cur else { return };
        if i + 1 == path.len() {
            o.insert(k.to_string(), value);
            break;
        }
        cur = o.entry(k.to_string()).or_insert(Value::Null);
    }
    if let Ok(mut s) = serde_json::to_string_pretty(&v) {
        s.push('\n');
        let _ = write_atomic(file, s.as_bytes());
    }
}

/// `[[ $s =~ ^#?[A-Fa-f0-9]{6}$ ]]`.
pub fn is_hex(s: &str) -> bool {
    let h = s.strip_prefix('#').unwrap_or(s);
    h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `${1##*.}` is one of the video extensions.
pub fn is_video(p: &str) -> bool {
    matches!(
        p.rsplit('.').next(),
        Some("mp4" | "webm" | "mkv" | "avi" | "mov")
    )
}

fn dirname(p: &str) -> String {
    match p.rfind('/') {
        None => ".".into(),
        Some(0) => "/".into(),
        Some(i) => p[..i].into(),
    }
}

fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// `<dir>/<name without -dark/-light>-<mode>.<ext>`, else the stripped name, when they exist.
pub fn mode_variant(imgpath: &str, mode: &str) -> String {
    let dir = dirname(imgpath);
    let base = basename(imgpath);
    let name = base.rfind('.').map_or(base, |i| &base[..i]);
    let ext = base.rsplit('.').next().unwrap_or(base);
    let stripped = name.strip_suffix("-dark").unwrap_or(name);
    let stripped = stripped.strip_suffix("-light").unwrap_or(stripped);
    let with_mode = format!("{dir}/{stripped}-{mode}.{ext}");
    let plain = format!("{dir}/{stripped}.{ext}");
    if Path::new(&with_mode).is_file() {
        with_mode
    } else if Path::new(&plain).is_file() {
        plain
    } else {
        imgpath.to_string()
    }
}

/// `hex_to_argb` of `generate_colors_material.py`: fixed slices that assume a leading `#`.
pub fn py_hex_to_argb(s: &str) -> Option<Argb> {
    let c: Vec<char> = s.chars().collect();
    let part = |a: usize, b: usize| -> Option<u8> {
        let t: String = c.get(a.min(c.len())..b.min(c.len()))?.iter().collect();
        u8::from_str_radix(t.trim(), 16).ok()
    };
    Some(Argb::new(
        255,
        part(1, 3)?,
        part(3, 5)?,
        part(5, c.len().max(5))?,
    ))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Color(String),
    Image(PathBuf),
}

/// Where the script's stdout and stderr go: line chunks ending in `\n`.
#[derive(Clone, Copy)]
pub struct Io<'a> {
    pub out: &'a (dyn Fn(String) + Sync),
    pub err: &'a (dyn Fn(String) + Sync),
}

fn print_out(s: String) {
    print!("{s}");
}

fn print_err(s: String) {
    elog::eprint!("{s}");
}

impl Io<'static> {
    /// The process's own stdout and stderr.
    pub const STD: Io<'static> = Io {
        out: &print_out,
        err: &print_err,
    };
}

macro_rules! r_out {
    ($r:expr, $($a:tt)*) => {
        ($r.io.out)(format!("{}\n", format_args!($($a)*)))
    };
}

macro_rules! r_err {
    ($r:expr, $($a:tt)*) => {
        ($r.io.err)(format!("{}\n", format_args!($($a)*)))
    };
}

struct Run<'a> {
    p: &'a Paths,
    sys: &'a Arc<dyn Sys>,
    io: Io<'a>,
}

impl Run<'_> {
    fn cfg(&self, path: &[&str]) -> Option<String> {
        jq_r(&std::fs::read_to_string(self.p.shell_config()).ok()?, path)
    }
    /// Saves `color` as the accent color and makes it the one this run uses: the shell's
    /// Config saves it to the file only after the run, where reading it back gave the old one.
    fn set_accent(&self, s: &mut State, color: &str) {
        self.sys.set_config(
            &self.p.shell_config(),
            &["appearance", "palette", "accentColor"],
            color,
        );
        s.accent = Some(color.to_string());
    }
    fn monitors(&self) -> Vec<Value> {
        match self.sys.hypr_json("monitors") {
            Some(Value::Array(a)) => a,
            _ => Vec::new(),
        }
    }

    /// `--color` without a color: the one the user picks on the screen.
    fn pick_accent(&self, s: &mut State) {
        let picked = self
            .sys
            .run(&["hyprpicker".into(), "--no-fancy".into()], None)
            .unwrap_or_default();
        self.set_accent(s, picked.split_whitespace().next().unwrap_or(""));
    }

    /// The configured wallpaper, empty when there is none.
    fn wallpaper_in_use(&self) -> String {
        match self.cfg(&["background", "wallpaperPath"]).as_deref() {
            None | Some("null" | "false") => String::new(),
            Some(v) => v.to_string(),
        }
    }

    fn resolve_noswitch(&self, s: &mut State) -> bool {
        let g = self.p.generated();
        let seed: String = first_line(&g.join("color.txt"))
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if is_hex(&seed) {
            s.imgpath.clear();
            s.color = Some(if seed.starts_with('#') {
                seed
            } else {
                format!("#{seed}")
            });
            return true;
        }
        let last = first_line(&g.join("wallpaper/path.txt"))
            .unwrap_or_default()
            .replace('\r', "");
        let last = last.strip_prefix("file://").unwrap_or(&last).to_string();
        if !last.is_empty() && Path::new(&last).is_file() && !is_video(&last) {
            s.imgpath = last;
            s.colorgen_only = true;
            return true;
        }
        let def = self
            .p
            .qs_config_dir()
            .join("assets/images/default_wallpaper.png");
        if def.is_file() {
            s.imgpath = def.to_string_lossy().into_owned();
            s.colorgen_only = true;
            return true;
        }
        false
    }

    fn pick_wallpaper(&self) -> Option<String> {
        pick(self.p, self.sys.as_ref())
    }

    fn create_restore(&self, video: &str) {
        let script = format!(
            "#!/bin/bash\n# Generated by switchwall.sh - Don't modify it by yourself.\n# Time: {}\n\npkill -9 -x mpvpaper\ntimeout 2 pidwait -x mpvpaper\n\nfor monitor in $(hyprctl monitors -j | jq -r '.[] | .name'); do\n    mpvpaper -o {} \"$monitor\" {} &\ndone\n",
            date(),
            sh_quote(VIDEO_OPTS),
            sh_quote(video)
        );
        let p = self.p.restore_script();
        if write_in_dir(&p, script.as_bytes()) {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(m) = std::fs::metadata(&p) {
                let mut perm = m.permissions();
                perm.set_mode(perm.mode() | 0o111);
                let _ = std::fs::set_permissions(&p, perm);
            }
        }
    }

    fn remove_restore(&self) {
        write_in_dir(
            &self.p.restore_script(),
            b"#!/bin/bash\n# The content of this script will be generated by switchwall.sh - Don't modify it by yourself.\n",
        );
    }
}

/// `cat > f.tmp << EOF ... && mv f.tmp f`: nothing when the directory is missing.
fn write_in_dir(p: &Path, data: &[u8]) -> bool {
    p.parent().is_some_and(Path::is_dir) && write_atomic(p, data).is_ok()
}

/// One shell word with the literal value of `s`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `date` in the C locale.
fn date() -> String {
    let mut buf = [0u8; 128];
    // SAFETY: time(2) with a null pointer only returns the time.
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY: `tm` is plain C data, valid all-zero.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r reads `t` and writes `tm`, both live locals (the reentrant form).
    unsafe { libc::localtime_r(&t, &mut tm) };
    // SAFETY: `buf` has `buf.len()` writable bytes, the format is NUL-terminated and `tm` was
    // filled above; strftime returns how many bytes it wrote (0 if they don't fit).
    let n = unsafe {
        libc::strftime(
            buf.as_mut_ptr().cast(),
            buf.len(),
            c"%a %b %e %H:%M:%S %Z %Y".as_ptr(),
            &tm,
        )
    };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

#[derive(Default, Debug)]
struct State {
    imgpath: String,
    mode: String,
    type_flag: String,
    color: Option<String>,
    /// The accent color this run set, which the config file may not have yet.
    accent: Option<String>,
    noswitch: bool,
    colorgen_only: bool,
}

/// What a switchwall run is asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    /// The wallpaper to switch to. Without one, and without [`Request::keep_wallpaper`] or an
    /// accent color in use, the run asks for it.
    pub image: String,
    /// `dark` or `light`; empty keeps the mode in use.
    pub mode: String,
    /// The scheme type (`scheme-tonal-spot`, `auto`, ...); empty takes the configured one.
    pub scheme: String,
    /// What to do with the accent color the colors are made from instead of the wallpaper.
    pub accent: Option<Accent>,
    /// Make the colors again for the wallpaper in use instead of switching (`--noswitch`):
    /// [`Request::image`] when it is given, the configured one otherwise.
    pub keep_wallpaper: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Accent {
    /// This color (`#rrggbb`).
    Set(String),
    /// None: back to the wallpaper's colors.
    Clear,
    /// The color the user picks on the screen (hyprpicker).
    Pick,
}

/// Entry point: `switchwall.sh <args>`; returns the exit status.
pub fn main(args: &[String], p: &Paths, sys: &Arc<dyn Sys>) -> i32 {
    main_io(args, p, sys, Io::STD)
}

/// [`main`] with the script's output handed to `io`.
pub fn main_io(args: &[String], p: &Paths, sys: &Arc<dyn Sys>, io: Io) -> i32 {
    let r = Run { p, sys, io };
    let mut s = State::default();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                s.mode = arg(i + 1);
                i += 2;
            }
            "--type" => {
                s.type_flag = arg(i + 1);
                i += 2;
            }
            "--color" => {
                let next = arg(i + 1);
                if is_hex(&next) {
                    r.set_accent(&mut s, &next);
                    i += 2;
                } else if next == "clear" {
                    r.set_accent(&mut s, "");
                    i += 2;
                } else {
                    r.pick_accent(&mut s);
                    i += 1;
                }
            }
            "--image" => {
                s.imgpath = arg(i + 1);
                i += 2;
            }
            "--noswitch" => {
                s.noswitch = true;
                s.imgpath = r.wallpaper_in_use();
                i += 1;
            }
            other => {
                if s.imgpath.is_empty() {
                    s.imgpath = other.to_string();
                }
                i += 1;
            }
        }
    }
    finish(&r, s)
}

/// The run `req` asks for, its output handed to `io`; returns the exit status of
/// `switchwall.sh` for the same request.
pub fn request_io(req: &Request, p: &Paths, sys: &Arc<dyn Sys>, io: Io) -> i32 {
    let r = Run { p, sys, io };
    let mut s = State {
        imgpath: req.image.clone(),
        mode: req.mode.clone(),
        type_flag: req.scheme.clone(),
        ..Default::default()
    };
    if req.keep_wallpaper {
        s.noswitch = true;
        // A caller that has just set the wallpaper names it: the shell's Config saves it to
        // the file only after the run, where reading it back gave the old one.
        if s.imgpath.is_empty() {
            s.imgpath = r.wallpaper_in_use();
        }
    }
    match &req.accent {
        Some(Accent::Set(color)) => r.set_accent(&mut s, color),
        Some(Accent::Clear) => r.set_accent(&mut s, ""),
        Some(Accent::Pick) => r.pick_accent(&mut s),
        None => {}
    }
    finish(&r, s)
}

/// The run once what it was asked for is in `s`.
fn finish(r: &Run, mut s: State) -> i32 {
    let p = r.p;
    let config_color = s.accent.take().unwrap_or_else(|| {
        r.cfg(&["appearance", "palette", "accentColor"])
            .unwrap_or_default()
    });
    if is_hex(&config_color) {
        s.color = Some(config_color);
    }
    let type_from_config = s.type_flag.is_empty();
    if type_from_config {
        s.type_flag = r
            .cfg(&["appearance", "palette", "type"])
            .unwrap_or_else(|| "auto".into());
    }
    if !ALLOWED_TYPES.contains(&s.type_flag.as_str()) {
        r_err!(
            r,
            "[switchwall.sh] Warning: Invalid type '{}', defaulting to 'auto'",
            s.type_flag
        );
        s.type_flag = "auto".into();
    }
    if s.noswitch && s.color.is_none() {
        if Path::new(&s.imgpath).is_file() && !is_video(&s.imgpath) {
            // The wallpaper stays: no upscale check, no mpvpaper restart, no config rewrite
            s.colorgen_only = true;
        } else if !r.resolve_noswitch(&mut s) {
            r_err!(
                r,
                "[switchwall.sh] --noswitch: no wallpaper or seed color to generate colors from"
            );
            return 1;
        }
        // An explicit `--type auto` re-detects; only the configured 'auto' keeps the scheme in use
        if type_from_config
            && s.type_flag == "auto"
            && let Some(last) = first_line(&p.generated().join("scheme_type.txt"))
        {
            let last: String = last.chars().filter(|c| !c.is_whitespace()).collect();
            if last != "auto" && ALLOWED_TYPES.contains(&last.as_str()) {
                s.type_flag = last;
            }
        }
    }
    if s.imgpath.is_empty() && s.color.is_none() && !s.noswitch {
        match r.pick_wallpaper() {
            Some(path) => s.imgpath = path,
            None => return 1,
        }
    }
    if !s.imgpath.is_empty() && !s.noswitch {
        r.set_accent(&mut s, "");
        s.color = None;
    }
    if s.type_flag == "auto" {
        if !s.imgpath.is_empty() && Path::new(&s.imgpath).is_file() {
            let detected = auto_scheme(Path::new(&s.imgpath));
            s.type_flag = detected.into();
        } else {
            r_err!(
                r,
                "[switchwall] Warning: No image to auto-detect scheme from, defaulting to 'scheme-tonal-spot'"
            );
            s.type_flag = "scheme-tonal-spot".into();
        }
    }
    if s.mode == "dark" || s.mode == "light" {
        s.imgpath = mode_variant(&s.imgpath, &s.mode);
    }
    switch(r, s)
}

/// The "Upscale?" body when `img` is smaller than the largest monitor; `None` when it is not or
/// its size cannot be read (an unreadable file is no reason to ask).
fn upscale_body(monitors: &[Value], img: &str) -> Option<String> {
    let max = |k: &str| monitors.iter().filter_map(|m| m[k].as_i64()).max();
    let (min_w, min_h) = (max("width")?, max("height")?);
    if is_video(img) || !Path::new(img).is_file() {
        return None;
    }
    let (w, h) = image_size(img)?;
    (w < min_w || h < min_h).then(|| {
        format!("Image resolution ({w}x{h}) is lower than screen resolution ({min_w}x{min_h})")
    })
}

/// `identify -format "%w"` / `"%h"`: the size the file's content gives, whatever its extension
/// says; AVIF and HEIC through libheif, as ImageMagick reads them.
fn image_size(img: &str) -> Option<(i64, i64)> {
    let by_content = || {
        image::ImageReader::open(img)
            .ok()?
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()
    };
    if let Some((w, h)) = by_content() {
        return Some((w.into(), h.into()));
    }
    let (w, h) = heif::file_size(img).ok()?;
    Some((w as i64, h as i64))
}

fn upscale_prompt(p: &Paths, sys: &Arc<dyn Sys>, monitors: &[Value], img: &str) {
    let Some(body) = upscale_body(monitors, img) else {
        return;
    };
    let _ = p;
    let s = sys.clone();
    let open = move |key: Option<String>| {
        if key.as_deref() != Some("open_upscayl") {
            return;
        }
        if s.has_command("upscayl") {
            s.spawn(&["upscayl".into()]);
            return;
        }
        let s2 = s.clone();
        let install = move |key: Option<String>| {
            if key.as_deref() != Some("install_upscayl") {
                return;
            }
            s2.run(
                &["kitty", "-1", "yay", "-S", "upscayl-bin"].map(String::from),
                None,
            );
            if s2.has_command("upscayl") {
                s2.spawn(&["upscayl".into()]);
            }
        };
        s.notify(
            "Wallpaper switcher",
            Some("im.error"),
            "Install Upscayl?",
            "yay -S upscayl-bin",
            &[("install_upscayl", "Install Upscayl (Arch)")],
            Box::new(install),
        );
    };
    sys.notify(
        "Wallpaper switcher",
        None,
        "Upscale?",
        &body,
        &[("open_upscayl", "Open Upscayl")],
        Box::new(open),
    );
}

/// Everything the color generation writes, computed before any of it touches the disk.
pub struct Generated {
    /// Matugen template outputs, in config order.
    pub templates: Vec<(PathBuf, String)>,
    pub scss: String,
    /// `--cache`: the image seed of the terminal palette, `#RRGGBB` (a color source leaves
    /// matugen's `color.txt`).
    pub seed_hex: Option<String>,
}

#[derive(Debug)]
pub struct GenError(pub String);

/// matugen `--source-color-index 0 <source> --mode <mode> --type <type>` + `generate_colors_material.py`.
pub fn generate(
    p: &Paths,
    source: &Source,
    mode: &str,
    py_mode: &str,
    scheme: &str,
    opts: &crate::scss::TermOpts,
) -> Result<Generated, GenError> {
    let e = |m: String| GenError(m);
    let dark = match mode {
        "dark" => true,
        "light" => false,
        m => return Err(e(format!("invalid mode {m:?}"))),
    };
    let (mg_source, image) = match source {
        Source::Color(c) => (
            Argb::from_str(c).map_err(|_| e(format!("invalid hex color {c:?}")))?,
            None,
        ),
        Source::Image(path) => {
            let canon =
                std::fs::canonicalize(path).map_err(|x| e(format!("{}: {x}", path.display())))?;
            (
                image_sources(path, &canon)?.0,
                Some(canon.to_string_lossy().into_owned()),
            )
        }
    };
    let colors = crate::mg::Colors::new(mg_source, crate::mg::variant(Some(scheme)), dark);
    let mut templates = Vec::new();
    if let Ok(cfg) = std::fs::read_to_string(p.config_home.join("matugen/config.toml")) {
        for t in crate::mg::templates(&cfg, &p.home).map_err(|x| e(x.0))? {
            let src = std::fs::read_to_string(&t.input)
                .map_err(|x| e(format!("{}: {x}", t.input.display())))?;
            templates.push((
                t.output,
                crate::mg::render(&src, &colors, image.as_deref()).map_err(|x| e(x.0))?,
            ));
        }
    }
    let py_dark = match py_mode {
        "dark" => true,
        "light" => false,
        m => return Err(e(format!("invalid mode {m:?}"))),
    };
    let seed = match source {
        Source::Color(c) => {
            py_hex_to_argb(c).ok_or_else(|| e(format!("invalid hex color {c:?}")))?
        }
        Source::Image(path) => {
            let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            image_sources(path, &canon)?.1
        }
    };
    let term_path = p.script_dir.join("terminal/scheme-base.json");
    let term: Value = serde_json::from_str(
        &std::fs::read_to_string(&term_path)
            .map_err(|x| e(format!("{}: {x}", term_path.display())))?,
    )
    .map_err(|x| e(x.to_string()))?;
    let scss = crate::scss::generate(seed, scheme, py_dark, Some(&term), opts)
        .map_err(|_| e("zero temperature range".into()))?;
    if !apply::valid_scss(&scss) {
        return Err(e("no $primary in scss".into()));
    }
    Ok(Generated {
        templates,
        scss,
        seed_hex: matches!(source, Source::Image(_))
            .then(|| format!("#{:02X}{:02X}{:02X}", seed.red, seed.green, seed.blue)),
    })
}

/// `colorfulness::scheme_for_image`, remembered per wallpaper like [`image_sources`]: a
/// light/dark switch with the "auto" palette otherwise decodes the wallpaper again.
fn auto_scheme(path: &Path) -> &'static str {
    use std::os::unix::fs::MetadataExt;
    static LAST: std::sync::Mutex<Option<(String, &'static str)>> = std::sync::Mutex::new(None);
    let key = std::fs::canonicalize(path)
        .ok()
        .zip(std::fs::metadata(path).ok())
        .map(|(c, m)| {
            format!(
                "{}\n{} {} {}",
                c.display(),
                m.mtime(),
                m.mtime_nsec(),
                m.len()
            )
        });
    let file = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(d).join("impulse-shell-colorgen-scheme"));
    let known = |v: &str| {
        ["scheme-neutral", "scheme-tonal-spot"]
            .into_iter()
            .find(|s| *s == v)
    };
    if let Some(k) = &key {
        if let Some((lk, v)) = LAST.lock().unwrap().as_ref()
            && lk == k
        {
            return v;
        }
        if let Some(f) = &file
            && let Ok(text) = std::fs::read_to_string(f)
            && let Some(rest) = text.strip_prefix(k.as_str())
            && let Some(v) = known(rest.trim())
        {
            *LAST.lock().unwrap() = Some((k.clone(), v));
            return v;
        }
    }
    let v = crate::colorfulness::scheme_for_image(path);
    if let Some(k) = key {
        if let Some(f) = &file {
            let _ = write_atomic(f, format!("{k}\n{v}\n").as_bytes());
        }
        *LAST.lock().unwrap() = Some((k, v));
    }
    v
}

/// matugen's source and the terminal seed of an image. Decoding a large wallpaper takes over a
/// second, so a light/dark or scheme switch on the same wallpaper reuses the last result:
/// kept in memory and in `$XDG_RUNTIME_DIR` (shared by the shell and the settings window).
fn image_sources(path: &Path, canon: &Path) -> Result<(Argb, Argb), GenError> {
    use std::os::unix::fs::MetadataExt;
    static LAST: std::sync::Mutex<Option<(String, Argb, Argb)>> = std::sync::Mutex::new(None);
    let key = std::fs::metadata(path).ok().map(|m| {
        format!(
            "{}\n{} {} {}",
            canon.display(),
            m.mtime(),
            m.mtime_nsec(),
            m.len()
        )
    });
    let file = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(d).join("impulse-shell-colorgen-sources"));
    let hex = |c: Argb| format!("#{:02X}{:02X}{:02X}", c.red, c.green, c.blue);
    if let Some(k) = &key {
        if let Some((lk, a, b)) = LAST.lock().unwrap().as_ref()
            && lk == k
        {
            return Ok((*a, *b));
        }
        if let Some(f) = &file
            && let Ok(text) = std::fs::read_to_string(f)
            && let Some(rest) = text.strip_prefix(k.as_str())
            && let Some((a, b)) = rest.trim().split_once(' ')
            && let (Some(a), Some(b)) = (py_hex_to_argb(a), py_hex_to_argb(b))
        {
            *LAST.lock().unwrap() = Some((k.clone(), a, b));
            return Ok((a, b));
        }
    }
    let mg = crate::mg::source_from_image(path).map_err(|x| GenError(x.0))?;
    let seed = crate::seed::seed_from_path(path).map_err(|x| GenError(x.0))?;
    if let Some(k) = key {
        if let Some(f) = &file {
            let _ = write_atomic(f, format!("{k}\n{} {}\n", hex(mg), hex(seed)).as_bytes());
        }
        *LAST.lock().unwrap() = Some((k, mg, seed));
    }
    Ok((mg, seed))
}

fn write_generated(p: &Paths, sys: &dyn Sys, g: &Generated, scheme: &str) -> std::io::Result<()> {
    for (out, text) in &g.templates {
        if crate::hyprlive::is_hypr_lua(&p.config_home, out) {
            crate::hyprlive::write(sys, out, text)?;
        } else {
            write_atomic(out, text.as_bytes())?;
        }
    }
    let gen_dir = p.generated();
    write_atomic(&gen_dir.join("material_colors.scss"), g.scss.as_bytes())?;
    if let Some(hex) = &g.seed_hex {
        write_atomic(&gen_dir.join("color.txt"), hex.as_bytes())?;
    }
    write_atomic(
        &gen_dir.join("scheme_type.txt"),
        format!("{scheme}\n").as_bytes(),
    )
}

/// In-process `gemini-categorize-wallpaper.sh <img>` (stdout), set by the shell; unset runs
/// the script.
pub static CATEGORIZE: std::sync::OnceLock<fn(&Path, &str) -> Option<String>> =
    std::sync::OnceLock::new();

/// Generation of the latest `switch`: a categorization still running for an older wallpaper
/// must not overwrite the newer one's `category.txt`.
static CATEGORY_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `gemini-categorize-wallpaper.sh "$img" > category.txt &` on the pool's blocking threads.
/// The script's `>` truncates the file up front and leaves it empty or half-written while it
/// runs, and two quick wallpaper changes race to whichever finishes last; here the file is
/// replaced atomically and only by the latest switch.
pub fn categorize_later(
    generation: u64,
    file: PathBuf,
    categorize: impl FnOnce() -> Option<String> + Send + 'static,
) {
    static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    background(move || {
        let out = categorize().unwrap_or_default();
        let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
        if CATEGORY_GEN.load(std::sync::atomic::Ordering::Acquire) == generation {
            let _ = write_atomic(
                &file,
                format!("{}\n", out.trim_end_matches('\n')).as_bytes(),
            );
        }
    });
}

/// Background jobs (`cmd &` in the script) not finished yet.
static BACKGROUND: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

/// `f &`: runs `f` on the pool's blocking threads, counted until it ends (panicking or not) for
/// [`wait_background`].
fn background(f: impl FnOnce() + Send + 'static) {
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            let (m, cv) = &BACKGROUND;
            *m.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
            cv.notify_all();
        }
    }
    *BACKGROUND.0.lock().unwrap_or_else(|e| e.into_inner()) += 1;
    let done = Done;
    workpool::long(move || {
        let _done = done;
        f();
    });
}

/// The script's `wait`: blocks until every background job started so far (the upscale prompt,
/// the wallpaper categorization) has finished, for callers that must see their effects.
pub fn wait_background() {
    let (m, cv) = &BACKGROUND;
    drop(
        cv.wait_while(m.lock().unwrap_or_else(|e| e.into_inner()), |n| *n > 0)
            .unwrap_or_else(|e| e.into_inner()),
    );
}

/// Starts a new `switch` generation (what `switch` does first); for tests of `categorize_later`.
pub fn next_category_generation() -> u64 {
    CATEGORY_GEN.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1
}

fn switch(r: &Run, s: State) -> i32 {
    let (p, sys) = (r.p, r.sys);
    let img = s.imgpath.clone();
    let category = next_category_generation();
    if r.cfg(&["background", "widgets", "clock", "cookie", "aiStyling"])
        .as_deref()
        == Some("true")
    {
        let (sys, p, img) = (sys.clone(), p.clone(), img.clone());
        let file = p.generated().join("wallpaper/category.txt");
        categorize_later(category, file, move || {
            let script = p.script_dir.join("../ai/gemini-categorize-wallpaper.sh");
            match CATEGORIZE.get() {
                Some(f) => f(&script, &img),
                None => sys.run(&[script.to_string_lossy().into_owned(), img], None),
            }
        });
    }
    let source = if let Some(c) = &s.color {
        Source::Color(c.clone())
    } else {
        if img.is_empty() {
            r_out!(r, "Aborted");
            return 0;
        }
        if !s.colorgen_only {
            let (sys2, p2, img2, mons) = (sys.clone(), p.clone(), img.clone(), r.monitors());
            background(move || upscale_prompt(&p2, &sys2, &mons, &img2));
            sys.kill_exact_wait("mpvpaper", libc::SIGKILL);
        }
        if s.colorgen_only {
            Source::Image(img.clone().into())
        } else if is_video(&img) {
            let _ = std::fs::create_dir_all(p.thumbnail_dir());
            let missing: Vec<&str> = ["mpvpaper", "ffmpeg"]
                .into_iter()
                .filter(|c| !sys.has_command(c))
                .collect();
            if !missing.is_empty() {
                let list = missing.join(" ");
                r_out!(r, "Missing deps: {list}");
                r_out!(r, "Arch: sudo pacman -S {list}");
                let (s, body) = (sys.clone(), format!("Missing dependencies: {list}"));
                let install = move |action: Option<String>| {
                    if action.as_deref() != Some("install_arch") {
                        return;
                    }
                    s.run(
                        &[
                            "kitty".into(),
                            "-1".into(),
                            "sudo".into(),
                            "pacman".into(),
                            "-S".into(),
                            list,
                        ],
                        None,
                    );
                    if s.has_command("mpvpaper") && s.has_command("ffmpeg") {
                        s.notify(
                            "Wallpaper switcher",
                            None,
                            "Wallpaper switcher",
                            "Alright, try again!",
                            &[],
                            Box::new(|_| {}),
                        );
                    }
                };
                sys.notify(
                    "Wallpaper switcher",
                    Some("im.error"),
                    "Can't switch to video wallpaper",
                    &body,
                    &[("install_arch", "Install (Arch)")],
                    Box::new(install),
                );
                return 0;
            }
            sys.set_config(&p.shell_config(), &["background", "wallpaperPath"], &img);
            for m in r.monitors() {
                if let Some(name) = m["name"].as_str() {
                    sys.spawn(&[
                        "mpvpaper".into(),
                        "-o".into(),
                        VIDEO_OPTS.into(),
                        name.into(),
                        img.clone(),
                    ]);
                }
            }
            let thumb = p.thumbnail_dir().join(format!("{}.jpg", basename(&img)));
            let t = thumb.to_string_lossy().into_owned();
            sys.run(
                &["ffmpeg", "-y", "-i", &img, "-vframes", "1", &t].map(String::from),
                None,
            );
            sys.set_config(&p.shell_config(), &["background", "thumbnailPath"], &t);
            if thumb.is_file() {
                r.create_restore(&img);
                Source::Image(thumb)
            } else {
                r_out!(r, "Cannot create image to colorgen");
                r.remove_restore();
                return 1;
            }
        } else {
            sys.set_config(&p.shell_config(), &["background", "wallpaperPath"], &img);
            r.remove_restore();
            Source::Image(img.clone().into())
        }
    };

    let mode = if s.mode.is_empty() {
        if sys.gsettings_get(IFACE_SCHEME).as_deref() == Some("prefer-dark") {
            "dark"
        } else {
            "light"
        }
        .to_string()
    } else {
        s.mode.clone()
    };
    let py_mode = if r
        .cfg(&[
            "appearance",
            "wallpaperTheming",
            "terminalGenerationProps",
            "forceDarkMode",
        ])
        .as_deref()
        == Some("true")
    {
        "dark".to_string()
    } else {
        mode.clone()
    };

    match mode.as_str() {
        "dark" => {
            sys.gsettings_set(IFACE_SCHEME, "prefer-dark");
            sys.gsettings_set(IFACE_GTK, "adw-gtk3-dark");
        }
        "light" => {
            sys.gsettings_set(IFACE_SCHEME, "prefer-light");
            sys.gsettings_set(IFACE_GTK, "adw-gtk3");
        }
        _ => {}
    }
    let _ = std::fs::create_dir_all(p.cache_home.join("rustshell/user/generated"));
    if p.shell_config().is_file()
        && r.cfg(&["appearance", "wallpaperTheming", "enableAppsAndShell"])
            .as_deref()
            == Some("false")
    {
        r_out!(
            r,
            "App and shell theming disabled, skipping matugen and color generation"
        );
        return 0;
    }

    let mut opts = crate::scss::TermOpts {
        blend_bg_fg: true,
        ..Default::default()
    };
    let props = ["appearance", "wallpaperTheming", "terminalGenerationProps"];
    for (key, slot) in [
        ("harmony", &mut opts.harmony),
        ("harmonizeThreshold", &mut opts.harmonize_threshold),
        ("termFgBoost", &mut opts.term_fg_boost),
    ] {
        let v = r.cfg(&[props[0], props[1], props[2], key]);
        if let Some(v) = v.filter(|v| v != "null" && !v.is_empty()) {
            match v.parse::<f64>() {
                Ok(f) => *slot = f,
                Err(_) => {
                    r_err!(
                        r,
                        "[switchwall.sh] Color generation failed, keeping previous colors"
                    );
                    return 1;
                }
            }
        }
    }

    let generated = match generate(p, &source, &mode, &py_mode, &s.type_flag, &opts) {
        Ok(g) => g,
        Err(e) => {
            r_err!(
                r,
                "[switchwall.sh] Color generation failed, keeping previous colors ({})",
                e.0
            );
            return 1;
        }
    };
    if let Err(e) = write_generated(p, &**sys, &generated, &s.type_flag) {
        r_err!(r, "[switchwall.sh] Writing colors failed: {e}");
        return 1;
    }

    let (state, qs, shell) = (p.state_dir(), p.qs_config_dir(), p.shell_config());
    apply::applycolor(
        &apply::Paths {
            script_dir: &p.script_dir,
            state_dir: &state,
            qs_config_dir: &qs,
            shell_config: &shell,
        },
        &**sys,
        r.io,
    );

    if r.cfg(&["appearance", "wallpaperTheming", "enableQtApps"])
        .as_deref()
        != Some("false")
    {
        let variant = if ALLOWED_TYPES[..8].contains(&s.type_flag.as_str()) {
            s.type_flag.as_str()
        } else {
            "scheme-tonal-spot"
        };
        let dark = || sys.gsettings_get(IFACE_SCHEME).as_deref() == Some("prefer-dark");
        kde_recolor(p, &**sys, dark, variant);
    }
    let color = std::fs::read_to_string(p.generated().join("color.txt")).unwrap_or_default();
    crate::code::apply(&p.config_home, color.trim_end_matches('\n'));
    0
}

const KDE_VARIANTS: [&str; 9] = [
    "scheme-content",
    "scheme-expressive",
    "scheme-fidelity",
    "scheme-monochrome",
    "scheme-neutral",
    "scheme-tonal-spot",
    "scheme-vibrant",
    "scheme-rainbow",
    "scheme-fruit-salad",
];

/// Recolours Qt/KDE apps in-process ([`crate::kde`]); replaces the original's
/// `kde-material-you-colors-wrapper.sh` (the Python kde-material-you-colors).
fn kde_recolor(p: &Paths, sys: &dyn Sys, dark: impl Fn() -> bool, variant: &str) {
    // Unknown/custom variants get Material's default, tonal spot.
    let sv = KDE_VARIANTS.iter().position(|v| *v == variant).unwrap_or(5);
    let Ok(color) = std::fs::read_to_string(p.generated().join("color.txt")) else {
        return;
    };
    let dirs = crate::kde::Dirs {
        config: p.config_home.clone(),
        data: match std::env::var("XDG_DATA_HOME") {
            Ok(v) if !v.is_empty() => PathBuf::from(v),
            _ => p.home.join(".local/share"),
        },
        cache: p.cache_home.clone(),
    };
    if let crate::kde::Outcome::Failed(e) = sys.kde_recolor(&dirs, color.trim(), dark(), sv) {
        elog::eprintln!("kde recolour: {e}");
    }
}

/// The wallpaper prompt's command, run in [`pick_dir`].
pub const PICK_ARGV: [&str; 5] = [
    "kdialog",
    "--getopenfilename",
    ".",
    "--title",
    "Choose wallpaper",
];

/// `$(xdg-user-dir PICTURES)`: the XDG pictures directory, else `home`.
pub fn pictures_dir(home: &Path) -> PathBuf {
    dirs::picture_dir().unwrap_or_else(|| home.to_path_buf())
}

/// Where switchwall.sh's prompt opens: the first of `<pics>/Wallpapers/showcase`,
/// `<pics>/Wallpapers` and `pics` (`$(xdg-user-dir PICTURES)`) that exists.
pub fn pick_dir(pics: PathBuf) -> Option<PathBuf> {
    [
        pics.join("Wallpapers/showcase"),
        pics.join("Wallpapers"),
        pics,
    ]
    .into_iter()
    .find(|d| d.is_dir())
}

/// switchwall.sh's wallpaper prompt: [`PICK_ARGV`] in [`pick_dir`]; the chosen path without
/// its newline (empty when the dialog was cancelled), `None` when none of the folders exists.
pub fn pick(p: &Paths, sys: &dyn Sys) -> Option<String> {
    let dir = pick_dir(pictures_dir(&p.home))?;
    Some(
        sys.run(&PICK_ARGV.map(String::from), Some(&dir))
            .unwrap_or_default()
            .trim_end_matches('\n')
            .to_string(),
    )
}

/// A place in the one line of in-process switchwall runs, taken when the run is requested. Runs
/// go one at a time in the order their turns were taken, so the last request's wallpaper and
/// colors are the ones left, whichever thread gets to run first.
#[must_use]
pub struct Turn(u64);

static TURNS: AtomicU64 = AtomicU64::new(0);
static SERVING: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());

impl Turn {
    pub fn take() -> Turn {
        Turn(TURNS.fetch_add(1, Ordering::SeqCst))
    }
}

/// Hands the line to the next turn when a run ends, panicking or not.
struct Serving;

impl Drop for Serving {
    fn drop(&mut self) {
        let (m, cv) = &SERVING;
        *m.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        cv.notify_all();
    }
}

/// Waits for `turn`; the line moves on when the result is dropped.
fn wait_for(turn: Turn) -> Serving {
    let (m, cv) = &SERVING;
    drop(
        cv.wait_while(m.lock().unwrap_or_else(|e| e.into_inner()), |s| {
            *s != turn.0
        })
        .unwrap_or_else(|e| e.into_inner()),
    );
    Serving
}

/// The run `req` asks for, in-process on the calling thread once `turn` comes; returns its
/// exit status.
pub fn run_in_turn(turn: Turn, req: &Request, p: &Paths, sys: &Arc<dyn Sys>, io: Io) -> i32 {
    let _next = wait_for(turn);
    request_io(req, p, sys, io)
}

/// The run `req` asks for, in-process on the calling thread after every run requested before
/// it; returns its exit status.
pub fn run(req: &Request, p: &Paths, sys: &Arc<dyn Sys>, io: Io) -> i32 {
    run_in_turn(Turn::take(), req, p, sys, io)
}

#[cfg(test)]
mod upscale_tests {
    use super::upscale_body;
    use serde_json::json;

    #[test]
    fn asks_only_for_readable_images_smaller_than_the_largest_monitor() {
        let dir = std::env::temp_dir().join(format!("upscale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mons = [
            json!({"width": 1920, "height": 1080}),
            json!({"width": 2560, "height": 1440}),
        ];
        let small = dir.join("small.png");
        image::RgbImage::new(1920, 1080).save(&small).unwrap();
        let big = dir.join("big.png");
        image::RgbImage::new(2560, 1440).save(&big).unwrap();
        let junk = dir.join("junk.png");
        std::fs::write(&junk, b"not an image").unwrap();
        assert_eq!(
            upscale_body(&mons, small.to_str().unwrap()).as_deref(),
            Some("Image resolution (1920x1080) is lower than screen resolution (2560x1440)")
        );
        assert_eq!(upscale_body(&mons, big.to_str().unwrap()), None);
        assert_eq!(upscale_body(&mons, junk.to_str().unwrap()), None);
        assert_eq!(
            upscale_body(&mons, dir.join("missing.png").to_str().unwrap()),
            None
        );
        assert_eq!(upscale_body(&[], small.to_str().unwrap()), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `identify` reads the size from the content: AVIF and HEIC too, whatever the file is
    /// called.
    #[test]
    fn size_comes_from_the_content() {
        let dir = tempfile::tempdir().unwrap();
        let mons = [json!({"width": 1920, "height": 1080})];
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../qsg/tests/fixtures");
        for (file, size) in [
            ("formats/rgb420.avif", "200x150"),
            ("heif/quad.heic", "32x16"),
            ("heif/rot.avif", "16x32"),
        ] {
            assert_eq!(
                upscale_body(&mons, fixtures.join(file).to_str().unwrap()),
                Some(format!(
                    "Image resolution ({size}) is lower than screen resolution (1920x1080)"
                )),
                "{file}"
            );
        }
        let png_called_jpg = dir.path().join("wall.jpg");
        image::RgbImage::new(640, 480)
            .save_with_format(&png_called_jpg, image::ImageFormat::Png)
            .unwrap();
        let avif_called_png = dir.path().join("wall.png");
        std::fs::copy(fixtures.join("formats/rgb420.avif"), &avif_called_png).unwrap();
        for (file, size) in [(png_called_jpg, "640x480"), (avif_called_png, "200x150")] {
            assert_eq!(
                upscale_body(&mons, file.to_str().unwrap()),
                Some(format!(
                    "Image resolution ({size}) is lower than screen resolution (1920x1080)"
                )),
                "{}",
                file.display()
            );
        }
    }
}
