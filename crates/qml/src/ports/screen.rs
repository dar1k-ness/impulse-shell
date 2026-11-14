//! The screen lightness probe behind `Screenshot.measureLightness` (`grim -o S - | magick png:-
//! -colorspace Gray -format "%[fx:mean*100]" info:` in the original) and the MangoHud config update
//! behind `MangoHud.setFpsLimit` (`grep`/`sed -i`/`echo >>` + `pkill -SIGUSR2 mangohud` in
//! The original); see [`crate::screenshot`].
//!
//! Where the original's commands were wrong this does the intended thing instead: the MangoHud config is
//! written through symlinks, its directory is created, and an appended key starts on its own
//! line.

use image::RgbaImage;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// `pkill -SIGUSR2 mangohud` after the config update.
    FpsReload,
}

impl Job {
    pub fn run(self, _io: &super::Io) -> i32 {
        match self {
            Job::FpsReload => {
                crate::osutil::pkill(libc::SIGUSR2, "mangohud");
                0
            }
        }
    }
}

/// What the probe pipeline prints for `output`'s frame now; `None` when it fails or `stop`
/// ended the wait for the frame.
pub fn lightness_of(output: &str, stop: &crate::svc::Stop) -> Option<String> {
    let (out, code) = lightness_probe(output, |o| super::wl::capture_or_stop(o, stop));
    (code == 0).then_some(out)
}

/// Stdout and exit status of the probe pipeline; `capture` stands in for `grim -o`.
pub fn lightness_probe(
    output: &str,
    capture: impl FnOnce(Option<&str>) -> Result<RgbaImage, String>,
) -> (String, i32) {
    match capture(Some(output)) {
        Ok(img) if img.width() > 0 && img.height() > 0 => (lightness(&img), 0),
        // `magick` reading an empty stdin: no output, status 1.
        _ => (String::new(), 1),
    }
}

/// `magick png:- -colorspace Gray -format "%[fx:mean*100]" info:` (ImageMagick 7, Q16 HDRI)
/// on the PNG grim writes for `img`: RGB when every pixel is opaque, RGBA otherwise.
pub fn lightness(img: &RgbaImage) -> String {
    let opaque = img.pixels().all(|p| p[3] == 255);
    let q = |c: u8| c as f64 * 257.0;
    let (mut gray, mut alpha) = (0.0f64, 0.0f64);
    for p in img.pixels() {
        // GRAYColorspace: Rec.709 luma on the stored (non-linear) values, kept as a float
        // Quantum.
        let g = 0.212656 * q(p[0]) + 0.715158 * q(p[1]) + 0.072186 * q(p[2]);
        gray += g.clamp(0.0, 65535.0) as f32 as f64;
        alpha += q(p[3]);
    }
    let n = (img.width() as u64 * img.height() as u64) as f64;
    // The composite mean averages the per-channel means (gray, and alpha when present).
    let mean = if opaque {
        gray / n
    } else {
        (gray / n + alpha / n) / 2.0
    };
    fmt_g(mean * (1.0 / 65535.0) * 100.0)
}

/// C `printf("%.6g", x)` for finite `x`.
fn fmt_g(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let e = format!("{x:.5e}");
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if !(-4..6).contains(&exp) {
        let m = mant.trim_end_matches('0').trim_end_matches('.');
        return format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    let s = format!("{:.*}", (5 - exp) as usize, x);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// bash's `~`: `$HOME` when set (even empty), else the passwd entry.
fn home() -> std::ffi::OsString {
    if let Some(h) = std::env::var_os("HOME") {
        return h;
    }
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: getuid(2) takes no arguments and cannot fail.
    let uid = unsafe { libc::getuid() };
    crate::osutil::passwd(crate::osutil::PwKey::Uid(uid), |e| {
        e.dir()
            .map(|d| std::ffi::OsStr::from_bytes(d.to_bytes()).to_os_string())
    })
    .ok()
    .flatten()
    .flatten()
    .unwrap_or_else(|| "~".into())
}

/// `~/.config/MangoHud/MangoHud.conf`.
pub fn mangohud_config() -> std::path::PathBuf {
    let mut cfg = home();
    cfg.push("/.config/MangoHud/MangoHud.conf");
    cfg.into()
}

const KEY: &[u8] = b"fps_limit=";

/// The original's `if grep -q '^fps_limit=' P; then sed -i 's/^fps_limit=.*/fps_limit=V/' P;
/// else echo 'fps_limit=V' >> P; fi`, without its flaws: a symlinked config (dotfile managers,
/// also a dangling link) is updated where it points instead of being replaced by a regular
/// file (atomically, mode and owner kept), a missing directory is created, the whole old value
/// goes (sed's `.*` stopped at the first byte that is not UTF-8), and an appended line never
/// joins a last line that has no newline. Every `fps_limit=` line gets the value; a line
/// ending's `\r` stays.
pub fn set_fps_limit(p: &Path, value: &str) -> std::io::Result<()> {
    let target = resolve_links(p);
    let data = match std::fs::read(&target) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(dir) = target.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            return std::fs::write(&target, format!("fps_limit={value}\n"));
        }
        Err(e) => return Err(e),
    };
    replace_file(&target, &with_fps_limit(&data, value))
}

/// `p` with every symlink followed, also a dangling one (its target gets created).
fn resolve_links(p: &Path) -> std::path::PathBuf {
    let mut cur = p.to_path_buf();
    for _ in 0..40 {
        match std::fs::read_link(&cur) {
            Ok(l) => {
                cur = match cur.parent() {
                    Some(d) if l.is_relative() => d.join(l),
                    _ => l,
                }
            }
            Err(_) => break,
        }
    }
    cur
}

/// The config bytes with `fps_limit=value` on every `fps_limit=` line, or appended on a line
/// of its own.
pub fn with_fps_limit(data: &[u8], value: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut found = false;
    let mut lines = data.split(|&b| b == b'\n').peekable();
    while let Some(l) = lines.next() {
        if l.starts_with(KEY) {
            found = true;
            out.extend_from_slice(KEY);
            out.extend_from_slice(value.as_bytes());
            if l.ends_with(b"\r") {
                out.push(b'\r');
            }
        } else {
            out.extend_from_slice(l);
        }
        if lines.peek().is_some() {
            out.push(b'\n');
        }
    }
    if !found {
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        out.extend_from_slice(KEY);
        out.extend_from_slice(value.as_bytes());
        out.push(b'\n');
    }
    out
}

/// Writes `data` to a new file next to `p` (same mode and owner) and renames it over `p`.
fn replace_file(p: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = std::fs::metadata(p)?;
    let dir = match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let mut tmp = tempfile::Builder::new()
        .prefix(".MangoHud")
        .rand_bytes(6)
        .permissions(std::fs::Permissions::from_mode(0o600))
        .tempfile_in(dir)?;
    let f = tmp.as_file_mut();
    let _ = std::os::unix::fs::fchown(&*f, Some(meta.uid()), Some(meta.gid()));
    f.set_permissions(std::fs::Permissions::from_mode(meta.mode() & 0o7777))?;
    f.write_all(data)?;
    f.sync_all()?;
    tmp.persist(p).map_err(|e| e.error)?;
    Ok(())
}
