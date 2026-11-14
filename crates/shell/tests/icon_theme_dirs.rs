//! Which directory of an icon theme a theme icon comes from, as Qt 6.12's QIconLoader picks it
//! (qtbase/src/gui/image/qiconloader.cpp).
//!
//! QIconTheme takes every section of `index.theme` that has a `Size` for a directory, listed
//! in `Directories` or not, in the order QSettings::allKeys() gives them: sorted. A valid
//! `icon-theme.cache` beside `index.theme` (not older than the theme's directory or any
//! directory it lists) replaces that order with the cache's own for the icon asked for, and an
//! icon the cache does not have is not in that theme at all. QIconLoader::findIconHelper puts
//! a PNG entry before the ones it has and an SVG entry after them, and
//! QIconLoaderEngine::entryForSize walks the entries: the first whose directory matches the
//! size and scale wins, else the closest, and of two equally close ones the order decides.
//!
//! Breeze's `application-x-executable` (Waffle's taskbar icon of a program without one) is
//! where it showed: `mimetypes/64` (Scalable, 64 to 256) is the last key sorted, so a 26 px
//! icon at a devicePixelRatio of 3 is that file under Quickshell; ours read `Directories` and
//! then `ScaledDirectories`, where `mimetypes/22@3x` comes later and took its place.
//!
//! Each file of the themes written here is a square of its directory's own colour, so the
//! colour in the middle of a `Kirigami.Icon` names the directory. Compared with what `qml6`
//! and Quickshell draw on a sway output of each scale.

use qsg::raster::Canvas;
use shell::scene::{self, Target};

const BG: [u8; 4] = [0x20, 0x20, 0x20, 0xff];
/// Side of each icon; the original's WAppIcon is 26.
const ICON: usize = 26;
const STEP: usize = 40;
const H: usize = 40;
/// The devicePixelRatio of the window: the output's scale.
const SCALES: [f64; 6] = [1.0, 1.5, 2.0, 3.0, 3.2, 4.0];

/// A directory of a theme: (theme, directory, the keys of its section, side of its files in
/// pixels, files). The order is the one `Directories` and `ScaledDirectories` list them in.
type Dir = (
    &'static str,
    &'static str,
    &'static str,
    usize,
    &'static [&'static str],
);

/// `u` is the user's theme: the directories of Breeze's `mimetypes` (`bz`), of Adwaita's
/// `places` (`adw`), two Fixed ones of one size (`tie` as PNG, `ties` as SVG), Fixed ones as
/// far from 26 on either side (`near`), two Threshold ones (`thr`) and a section no list
/// names (`nolist`). `p` is the theme it
/// inherits, AdwaitaLegacy's `places` (`leg`); `hicolor` has hicolor's `apps` (`hi`, `hi2`).
/// `late` is in `u` and in `p`; with a cache, `u`'s copy is written after the cache.
const DIRS: &[Dir] = &[
    (
        "u",
        "m/16",
        "Size=16\nContext=MimeTypes\nType=Fixed",
        16,
        &["bz.svg"],
    ),
    (
        "u",
        "m/22",
        "Size=22\nContext=MimeTypes\nType=Scalable\nMinSize=22\nMaxSize=24",
        22,
        &["bz.svg"],
    ),
    (
        "u",
        "m/32",
        "Size=32\nContext=MimeTypes\nType=Fixed",
        32,
        &["bz.svg"],
    ),
    (
        "u",
        "m/64",
        "Size=64\nContext=MimeTypes\nType=Scalable\nMinSize=64\nMaxSize=256",
        64,
        &["bz.svg", "late.svg"],
    ),
    (
        "u",
        "16x16/places",
        "Context=Places\nSize=16\nType=Fixed",
        16,
        &["adw.png"],
    ),
    (
        "u",
        "scalable/places",
        "Context=Places\nSize=128\nMinSize=8\nMaxSize=512\nType=Scalable",
        128,
        &["adw.svg"],
    ),
    (
        "u",
        "z28",
        "Size=28\nType=Fixed",
        28,
        &["tie.png", "ties.svg"],
    ),
    (
        "u",
        "a28",
        "Size=28\nType=Fixed",
        28,
        &["tie.png", "ties.svg", "near.png"],
    ),
    ("u", "n24", "Size=24\nType=Fixed", 24, &["near.png"]),
    (
        "u",
        "t20",
        "Size=20\nType=Threshold\nThreshold=4",
        20,
        &["thr.png"],
    ),
    ("u", "t32", "Size=32\nType=Threshold", 32, &["thr.png"]),
    (
        "u",
        "m/16@2x",
        "Size=16\nScale=2\nContext=MimeTypes\nType=Fixed",
        32,
        &["bz.svg"],
    ),
    (
        "u",
        "m/16@3x",
        "Size=16\nScale=3\nContext=MimeTypes\nType=Fixed",
        48,
        &["bz.svg"],
    ),
    (
        "u",
        "m/22@2x",
        "Size=22\nScale=2\nContext=MimeTypes\nType=Scalable\nMinSize=22\nMaxSize=24",
        44,
        &["bz.svg"],
    ),
    (
        "u",
        "m/22@3x",
        "Size=22\nScale=3\nContext=MimeTypes\nType=Scalable\nMinSize=22\nMaxSize=24",
        66,
        &["bz.svg"],
    ),
    ("u", "unlisted", "Size=26\nType=Fixed", 26, &["nolist.svg"]),
    (
        "p",
        "16x16/places",
        "Context=Places\nSize=16\nType=Fixed",
        16,
        &["leg.png"],
    ),
    (
        "p",
        "22x22/places",
        "Context=Places\nSize=22\nType=Fixed",
        22,
        &["leg.png"],
    ),
    (
        "p",
        "24x24/places",
        "Context=Places\nSize=24\nType=Fixed",
        24,
        &["leg.png"],
    ),
    (
        "p",
        "32x32/places",
        "Context=Places\nSize=32\nType=Fixed",
        32,
        &["leg.png", "late.png"],
    ),
    (
        "p",
        "48x48/places",
        "Context=Places\nSize=48\nType=Fixed",
        48,
        &["leg.png"],
    ),
    (
        "hicolor",
        "16x16/apps",
        "Size=16\nContext=Applications\nType=Threshold",
        16,
        &["hi.png", "hi2.png"],
    ),
    (
        "hicolor",
        "32x32/apps",
        "Size=32\nContext=Applications\nType=Threshold",
        32,
        &["hi.png", "hi2.png"],
    ),
    (
        "hicolor",
        "64x64/apps",
        "Size=64\nContext=Applications\nType=Threshold",
        64,
        &["hi.png"],
    ),
    (
        "hicolor",
        "128x128/apps",
        "Size=128\nContext=Applications\nType=Threshold",
        128,
        &["hi.png"],
    ),
    (
        "hicolor",
        "scalable/apps",
        "MinSize=1\nSize=128\nMaxSize=256\nContext=Applications\nType=Scalable",
        128,
        &["hi.svg"],
    ),
    (
        "hicolor",
        "16x16@2/apps",
        "Size=16\nScale=2\nContext=Applications\nType=Threshold",
        32,
        &["hi2.png"],
    ),
    (
        "hicolor",
        "32x32@2/apps",
        "Size=32\nScale=2\nContext=Applications\nType=Threshold",
        64,
        &["hi2.png"],
    ),
];

/// `u` once more, in `$XDG_DATA_HOME/icons`, which Qt searches before `$XDG_DATA_DIRS`: a
/// PNG and an SVG the other `u` has in the same directory, and `bz` in its last one.
const HOME: &[Dir] = &[
    ("u", "z28", "Size=28\nType=Fixed", 28, &["tie.png"]),
    ("u", "a28", "Size=28\nType=Fixed", 28, &["ties.svg"]),
    (
        "u",
        "m/64",
        "Size=64\nContext=MimeTypes\nType=Scalable\nMinSize=64\nMaxSize=256",
        64,
        &["bz.svg"],
    ),
];

/// `u` a third time, in `~/.icons`, which Qt searches first: the SVG and `bz` of [`HOME`],
/// and a PNG of `leg`, which the other two have only in the theme `u` inherits.
const DOT: &[Dir] = &[
    ("u", "z28", "Size=28\nType=Fixed", 28, &["leg.png"]),
    ("u", "a28", "Size=28\nType=Fixed", 28, &["ties.svg"]),
    (
        "u",
        "m/64",
        "Size=64\nContext=MimeTypes\nType=Scalable\nMinSize=64\nMaxSize=256",
        64,
        &["bz.svg"],
    ),
];

/// The icons drawn, left to right.
const NAMES: [&str; 11] = [
    "bz", "adw", "tie", "ties", "near", "thr", "nolist", "leg", "hi", "hi2", "late",
];

/// What Qt 6.12 and Quickshell 0.3.2 draw for `bz` without a cache at each of [`SCALES`].
const BREEZE: [&str; 6] = [
    "u/m/22",
    "u/m/22@2x",
    "u/m/22@2x",
    "u/m/64",
    "u/m/64",
    "u/m/64",
];

/// The colour of the files of directory `n` of [`DIRS`]: a channel differs by 85 and more
/// between any two.
fn colour(n: usize) -> [u8; 3] {
    assert!(n < 48);
    [
        (n % 4 * 85) as u8,
        (n / 4 % 4 * 85) as u8,
        [255, 128, 0][n / 16],
    ]
}

/// The directory whose colour `px` is; `~/` begins one of [`HOME`], `~/.icons/` one of
/// [`DOT`].
fn directory(px: [u8; 4]) -> String {
    let near = |c: [u8; 3]| (0..3).all(|k| c[k].abs_diff(px[k]) < 24);
    if near([BG[0], BG[1], BG[2]]) {
        return "nothing".into();
    }
    DIRS.iter()
        .chain(HOME)
        .chain(DOT)
        .enumerate()
        .find(|(n, _)| near(colour(*n)))
        .map(|(n, d)| {
            let place = (n >= DIRS.len()) as usize + (n >= DIRS.len() + HOME.len()) as usize;
            format!("{}{}/{}", ["", "~/", "~/.icons/"][place], d.0, d.1)
        })
        .unwrap_or_else(|| format!("#{:02x}{:02x}{:02x}", px[0], px[1], px[2]))
}

#[derive(Clone, Copy, PartialEq)]
enum Cache {
    /// No `icon-theme.cache`: Breeze as Arch ships it into the parity guest.
    None,
    /// One written by `gtk-update-icon-cache` and newer than every directory.
    Valid,
    /// One older than the theme's directory: Qt does not read it.
    Stale,
    /// One newer than the theme's directory and older than a directory it lists.
    StaleDir,
    /// The first half of a valid one: the list of directories is at its end.
    Truncated,
    /// A valid one whose header has the hash table at an odd offset: the first icon looked
    /// up finds that out.
    HashOffset,
    /// A valid one where the hash bucket of `near` points past the icons: `near` is not in
    /// the theme and the rest of the cache is read.
    Bucket,
    /// A valid one where that bucket is at an odd offset: Qt reads the cache until an icon
    /// of that bucket is asked for and not after it.
    OddBucket,
    /// A valid one where the list of images of `near` ends past the end of the file: the
    /// same, at `near`.
    ImagesPastEnd,
    /// A valid one whose list of directories has a name at the end of the file: not read.
    NameAtEnd,
}

/// What `$XDG_DATA_HOME/icons/u` is.
#[derive(Clone, Copy, PartialEq)]
enum Home {
    None,
    /// The directories of [`HOME`] and no `index.theme`.
    Files,
    /// Those and an `index.theme` of their sections alone, which is the one Qt reads.
    Index,
    /// The directories of [`HOME`], and the ones of [`DOT`] in `~/.icons/u`; no `index.theme`
    /// in either.
    FilesAndDotIcons,
}

fn write_file(n: usize, (theme, dir, _, side, _): &Dir, file: &str, icons: &std::path::Path) {
    let path = icons.join(theme).join(dir);
    std::fs::create_dir_all(&path).unwrap();
    let [r, g, b] = colour(n);
    if file.ends_with(".png") {
        let c = Canvas::new(*side, *side, [r, g, b, 255]);
        c.save_png(path.join(file).to_str().unwrap()).unwrap();
    } else {
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{side}\" height=\"{side}\">\
             <rect width=\"{side}\" height=\"{side}\" fill=\"#{r:02x}{g:02x}{b:02x}\"/></svg>\n"
        );
        std::fs::write(path.join(file), svg).unwrap();
    }
}

fn write_index(root: &std::path::Path, theme: &str, inherits: &str, dirs: &[Dir]) {
    let of = |scaled: bool| -> Vec<&str> {
        dirs.iter()
            .filter(|d| d.1 != "unlisted" && d.2.contains("Scale=") == scaled)
            .map(|d| d.1)
            .collect()
    };
    let mut index = format!(
        "[Icon Theme]\nName={theme}\nComment={theme}\n{inherits}Directories={}\n\
         ScaledDirectories={}\n",
        of(false).join(","),
        of(true).join(",")
    );
    for d in dirs {
        index += &format!("\n[{}]\n{}\n", d.1, d.2);
    }
    std::fs::write(root.join("index.theme"), index).unwrap();
}

fn update_cache(root: &std::path::Path) {
    let st = std::process::Command::new("gtk-update-icon-cache")
        .args(["-f", "-q"])
        .arg(root)
        .status()
        .expect("gtk-update-icon-cache");
    assert!(st.success(), "gtk-update-icon-cache {root:?}");
}

/// `cache` as `how` damages it. The file is a header (version, offset of the hash table,
/// offset of the list of directories), the hash table, the icons and the list.
fn damage(cache: &std::path::Path, how: Cache) {
    let mut data = std::fs::read(cache).unwrap();
    let u32_at =
        |d: &[u8], at: usize| u32::from_be_bytes(d[at..at + 4].try_into().unwrap()) as usize;
    match how {
        Cache::Truncated => data.truncate(data.len() / 2),
        Cache::HashOffset => data[7] |= 1,
        Cache::Bucket => {
            let bucket = bucket_of_near(&data);
            let past = (data.len() as u32 - 8).to_be_bytes();
            data[bucket..bucket + 4].copy_from_slice(&past);
        }
        Cache::OddBucket => {
            let bucket = bucket_of_near(&data);
            data[bucket + 3] |= 1;
        }
        Cache::ImagesPastEnd => {
            // An icon of the bucket: offset of the next one, of its name, of its images.
            let mut icon = u32_at(&data, bucket_of_near(&data));
            while &data[u32_at(&data, icon + 4)..][..5] != b"near\0" {
                icon = u32_at(&data, icon);
            }
            let images = u32_at(&data, icon + 8);
            let count = (data.len() as u32).to_be_bytes();
            data[images..images + 4].copy_from_slice(&count);
        }
        Cache::NameAtEnd => {
            let list = u32_at(&data, 8);
            let last = list + 4 * u32_at(&data, list);
            let end = (data.len() as u32).to_be_bytes();
            data[last..last + 4].copy_from_slice(&end);
        }
        _ => return,
    }
    std::fs::write(cache, data).unwrap();
}

/// Where the hash table of the cache `data` has the bucket of the icon `near`.
fn bucket_of_near(data: &[u8]) -> usize {
    let u32_at = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    let hash = "near"
        .bytes()
        .fold(0u32, |h, b| (h << 5).wrapping_sub(h).wrapping_add(b as u32));
    let table = u32_at(4);
    let bucket = table + 4 + (hash as usize % u32_at(table)) * 4;
    assert!(u32_at(bucket) < data.len(), "`near` is in the cache");
    bucket
}

fn write_themes(
    share: &std::path::Path,
    data_home: &std::path::Path,
    dot_icons: &std::path::Path,
    cache: Cache,
    home: Home,
) {
    let icons = share.join("icons");
    // `u`'s late.svg is not in a cache.
    let late = |d: &Dir, f: &str| cache != Cache::None && d.0 == "u" && f == "late.svg";
    for (n, d) in DIRS.iter().enumerate() {
        for f in d.4.iter().filter(|f| !late(d, f)) {
            write_file(n, d, f, &icons);
        }
    }
    let mut roots = Vec::new();
    for (theme, inherits) in [("u", "Inherits=p\n"), ("p", ""), ("hicolor", "")] {
        let dirs: Vec<Dir> = DIRS.iter().filter(|d| d.0 == theme).copied().collect();
        write_index(&icons.join(theme), theme, inherits, &dirs);
        roots.push(icons.join(theme));
    }
    if home != Home::None {
        for (n, d) in HOME.iter().enumerate() {
            write_file(DIRS.len() + n, d, d.4[0], &data_home.join("icons"));
        }
    }
    if home == Home::FilesAndDotIcons {
        for (n, d) in DOT.iter().enumerate() {
            write_file(DIRS.len() + HOME.len() + n, d, d.4[0], dot_icons);
        }
    }
    if home == Home::Index {
        let root = data_home.join("icons/u");
        write_index(&root, "u", "Inherits=p\n", HOME);
        roots.push(root);
    }
    if cache == Cache::None {
        return;
    }
    for root in &roots {
        update_cache(root);
    }
    for (n, d) in DIRS.iter().enumerate() {
        for f in d.4.iter().filter(|f| late(d, f)) {
            write_file(n, d, f, &icons);
        }
    }
    damage(&icons.join("u/icon-theme.cache"), cache);
    // Qt compares the cache's time with the theme's directory and with each directory in it.
    let now = std::time::SystemTime::now();
    let at = match cache {
        Cache::Stale => now - std::time::Duration::from_secs(3600),
        _ => now + std::time::Duration::from_secs(2),
    };
    let touch = |path: &std::path::Path, at| {
        let f = std::fs::File::open(path).unwrap();
        f.set_modified(at).unwrap();
    };
    for root in &roots {
        touch(&root.join("icon-theme.cache"), at);
    }
    if cache == Cache::StaleDir {
        touch(
            &icons.join("u/t20"),
            now + std::time::Duration::from_secs(4),
        );
    }
}

fn icon_qml(n: usize, name: &str, side: usize) -> String {
    format!(
        "    Kirigami.Icon {{ x: {}; y: 7; implicitWidth: {side}; implicitHeight: {side}; \
         roundToIconSize: false; source: \"{name}\" }}\n",
        n * STEP + 7
    )
}

/// The icons of [`NAMES`]; with `ends`, that icon once more before them and once more after
/// them, to their right: the first icon asked for whichever end the asking starts from.
fn scene_qml(ends: Option<&str>) -> (String, (usize, usize)) {
    let mut s = String::new();
    if let Some(name) = ends {
        s += &icon_qml(NAMES.len(), name, ICON);
    }
    for (n, name) in NAMES.iter().enumerate() {
        s += &icon_qml(n, name, ICON);
    }
    if let Some(name) = ends {
        s += &icon_qml(NAMES.len() + 1, name, ICON);
    }
    (s, ((NAMES.len() + 2) * STEP, H))
}

/// Side of the icon [`scene_again_qml`] asks again.
const AGAIN: usize = 16;

/// The icons of [`NAMES`] and, to their right, `name` once more with a side of [`AGAIN`]:
/// declared first, so it is the last one asked for.
fn scene_again_qml(name: &str) -> (String, (usize, usize)) {
    let (scene, size) = scene_qml(None);
    (icon_qml(NAMES.len(), name, AGAIN) + &scene, size)
}

fn swaymsg(sway: &testenv::Sway, cmd: &str) {
    let st = std::process::Command::new("swaymsg")
        .env("SWAYSOCK", sway.ipc_socket())
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .status()
        .expect("swaymsg");
    assert!(st.success(), "swaymsg {cmd}");
}

/// What draws the answer: `qml6`, or Quickshell (`qs`).
#[derive(Clone, Copy, PartialEq)]
enum Reference {
    Qml6,
    Quickshell,
}

/// What a sway output shows of the scene under `by` at each of `scales`, in device pixels:
/// one process, whose output is given the next scale once the one before is drawn.
fn reference_pixels(
    by: Reference,
    scene: &str,
    dir: &std::path::Path,
    (w, h): (usize, usize),
    scales: &[f64],
) -> Vec<Canvas> {
    let sway = testenv::Sway::start();
    swaymsg(&sway, "output HEADLESS-1 resolution 2560x1600");
    swaymsg(&sway, &format!("output HEADLESS-1 scale {}", scales[0]));
    swaymsg(&sway, "default_border none");
    let (imports, window, width, height) = match by {
        Reference::Qml6 => ("", "Window", "width", "height"),
        Reference::Quickshell => (
            "import Quickshell\n",
            "FloatingWindow",
            "implicitWidth",
            "implicitHeight",
        ),
    };
    // A frame is swapped once the compositor showed the one before it: by the tenth at a
    // ratio the output's scale reached the window and the icons were polished and are on
    // screen.
    let source = format!(
        "import QtQuick\n{imports}import org.kde.kirigami as Kirigami\n{window} {{\n    \
         {width}: {w}; {height}: {h}; visible: true; color: \"transparent\"\n    \
         Rectangle {{\n    id: box\n    width: {w}; height: {h}; color: \"#202020\"\n    \
         Connections {{\n        target: box.Window.window\n        \
         function onFrameSwapped() {{\n            \
         console.log(\"DPR\", box.Window.window.devicePixelRatio);\n            \
         box.Window.window.update();\n        }}\n    }}\n\
         {scene}    }}\n}}\n"
    );
    let shell = dir.join(match by {
        Reference::Qml6 => "qml6",
        Reference::Quickshell => "qs",
    });
    std::fs::create_dir_all(&shell).unwrap();
    std::fs::write(shell.join("shell.qml"), source).unwrap();
    let mut cmd = match by {
        Reference::Qml6 => {
            let mut c = std::process::Command::new("qml6");
            c.arg(shell.join("shell.qml"));
            c
        }
        Reference::Quickshell => {
            let mut c = std::process::Command::new("qs");
            c.arg("-p").arg(&shell);
            c
        }
    };
    let (r, wr) = std::io::pipe().unwrap();
    let mut child = sway
        .env(&mut cmd)
        // Quickshell refuses any platform name but `wayland`.
        .env(
            "QT_QPA_PLATFORM",
            match by {
                Reference::Qml6 => "wayland-egl",
                Reference::Quickshell => "wayland",
            },
        )
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_WAYLAND_DISABLE_WINDOWDECORATION", "1")
        .env_remove("QT_SCALE_FACTOR")
        .stdin(std::process::Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    let log = testenv::Log::capture(r);
    let mut shots = Vec::new();
    for (k, scale) in scales.iter().enumerate() {
        if k > 0 {
            swaymsg(&sway, &format!("output HEADLESS-1 scale {scale}"));
        }
        let ratio = format!("DPR {scale}\n");
        let shown = log.wait(|s| s.matches(&ratio).count() >= 10);
        let shot = shell.join(format!("{scale}.png"));
        let ok = shown.is_ok()
            && sway
                .env(&mut std::process::Command::new("grim"))
                .args(["-o", "HEADLESS-1"])
                .arg(&shot)
                .status()
                .is_ok_and(|s| s.success());
        if !ok {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "{:?} at scale {scale} printed:\n{}",
                cmd.get_program(),
                log.text()
            );
        }
        let full = qsg::image::load_png(shot.to_str().unwrap()).unwrap();
        let (dw, dh) = shell::gpu::device_size(w, h, *scale);
        let mut c = Canvas::new(dw, dh, BG);
        for y in 0..dh {
            c.px[y * dw..(y + 1) * dw].copy_from_slice(&full.px[y * full.w..y * full.w + dw]);
        }
        shots.push(c);
    }
    let _ = child.kill();
    let _ = child.wait();
    shots
}

/// The scene as the shell draws a window on a lone output at each of `scales`, in device
/// pixels: one window, whose output is given the next scale once the one before is drawn.
fn our_pixels(scene: &str, (w, h): (usize, usize), scales: &[f64]) -> Vec<Canvas> {
    let source = format!(
        "import QtQuick\nimport Quickshell\nimport org.kde.kirigami as Kirigami\n\
         FloatingWindow {{\n    width: {w}; height: {h}; color: \"#202020\"\n{scene}}}\n"
    );
    scene::set_screens_scale(scales[0].ceil() as i32);
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&source).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    let (mut gpu, _) =
        shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
    let mut shots = Vec::new();
    for scale in scales {
        scene::set_screens_scale(scale.ceil() as i32);
        let (dw, dh) = shell::gpu::device_size(w, h, *scale);
        let mut surface = gpu.pbuffer(dw as i32, dh as i32).expect("pbuffer");
        for _ in 0..3 {
            rt.settle();
            scene::layout_window(&mut rt, &mut fonts, win, [w as f64, h as f64]);
        }
        let mut fr = shell::gpu::Frame::scaled(w, h, *scale);
        scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
        gpu.render(&mut surface, &mut fr, || {});
        shots.push(gpu.read_surface(&surface));
    }
    shots
}

/// The directory each of the first `icons` icons is drawn from: the colour in its middle.
fn drawn(c: &Canvas, scale: f64, icons: usize) -> Vec<String> {
    (0..icons)
        .map(|n| {
            let side = if n < NAMES.len() { ICON } else { AGAIN };
            let mid = |v: usize| ((v + 7 + side / 2) as f64 * scale) as usize;
            directory(c.px[mid(0) * c.w + mid(n * STEP)])
        })
        .collect()
}

/// Whether this process is the one that runs `test`: with the tools, in a process of its own
/// whose icon theme is `u` with `cache`, under Qt's own QKdeTheme.
fn setup(test: &str, cache: Cache, home: Home) -> bool {
    setup_under(test, cache, home, false)
}

/// Where tools/testenv/Containerfile keeps plasma-integration's platform theme plugin.
const PLUGINS: &str = "/opt/plasma-integration/plugins";

/// [`setup`]; with `plugin` the platform theme is plasma-integration's, which a KDE desktop
/// has and whose theme icons KIconEngine answers.
fn setup_under(test: &str, cache: Cache, home: Home, plugin: bool) -> bool {
    if plugin
        && !std::path::Path::new(PLUGINS)
            .join("platformthemes")
            .is_dir()
    {
        testenv::unavailable(format_args!(
            "plasma-integration's platform theme plugin in {PLUGINS}"
        ));
        return false;
    }
    if !testenv::require_all(&[
        "qml6",
        "qs",
        "sway",
        "swaymsg",
        "grim",
        "gtk-update-icon-cache",
    ]) {
        return false;
    }
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let (config, share) = (dir.path().join(".config"), dir.path().join("share"));
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("kdeglobals"), "[Icons]\nTheme=u\n").unwrap();
        let mut vars = testenv::home_vars(dir.path());
        let data_home =
            std::path::PathBuf::from(&vars.iter().find(|v| v.0 == "XDG_DATA_HOME").unwrap().1);
        write_themes(&share, &data_home, &dir.path().join(".icons"), cache, home);
        vars.extend([
            ("QT_QPA_PLATFORMTHEME", "kde".into()),
            ("KDE_SESSION_VERSION", "6".into()),
            ("XDG_DATA_DIRS", share.into()),
        ]);
        let mut unset = vec!["XDG_CURRENT_DESKTOP", "KDEHOME", "QS_ICON_THEME"];
        if plugin {
            vars.push(("QT_PLUGIN_PATH", PLUGINS.into()));
            // The shell's own style (`pragma Env` of its entry points): with the plugin Quick
            // Controls would otherwise load the desktop's.
            vars.push(("QT_QUICK_CONTROLS_STYLE", "Basic".into()));
        } else {
            unset.push("QT_PLUGIN_PATH");
        }
        testenv::run_in_own_process_env(test, vars, &unset);
        return false;
    }
    true
}

/// The icons the shell draws from another directory than Qt at each of [`SCALES`]; `breeze`
/// says whether Qt's `bz` is to be the one of [`BREEZE`], `ends` is [`scene_qml`]'s.
fn differences(breeze: bool, ends: Option<&str>) -> Vec<String> {
    let wrong = differences_of(&scene_qml(ends), NAMES.len(), Process::EachScale, breeze);
    wrong.0
}

/// How long the process of Qt that draws the answers lives.
#[derive(Clone, Copy, PartialEq)]
enum Process {
    /// A new one at each of [`SCALES`].
    EachScale,
    /// One, whose output goes through [`SCALES`].
    AllScales,
}

/// [`differences`] of the first `icons` icons of a scene, and how many were compared. The
/// shell is one thread through all of [`SCALES`], and one window where Qt is one process.
fn differences_of(
    (scene, size): &(String, (usize, usize)),
    icons: usize,
    qt: Process,
    breeze: bool,
) -> (Vec<String>, usize) {
    let dir = tempfile::tempdir().unwrap();
    let name = |n: usize| NAMES.get(n).copied().unwrap_or("again");
    let process = qt;
    let reference = |by| -> Vec<Vec<String>> {
        let shots = match process {
            Process::AllScales => reference_pixels(by, scene, dir.path(), *size, &SCALES),
            Process::EachScale => SCALES
                .iter()
                .map(|s| reference_pixels(by, scene, dir.path(), *size, &[*s]).remove(0))
                .collect(),
        };
        shots
            .iter()
            .zip(SCALES)
            .map(|(c, scale)| drawn(c, scale, icons))
            .collect()
    };
    let qt = reference(Reference::Qml6);
    let qs = reference(Reference::Quickshell);
    let ours: Vec<Canvas> = match process {
        Process::AllScales => our_pixels(scene, *size, &SCALES),
        Process::EachScale => SCALES
            .iter()
            .map(|s| our_pixels(scene, *size, &[*s]).remove(0))
            .collect(),
    };
    let mut wrong = Vec::new();
    for (k, scale) in SCALES.iter().enumerate() {
        let ours = drawn(&ours[k], *scale, icons);
        if std::env::var_os("ICON_THEME_DIRS_PRINT").is_some() {
            for n in 0..icons {
                eprintln!(
                    "ROW {scale} {} qt {} qs {} ours {}",
                    name(n),
                    qt[k][n],
                    qs[k][n],
                    ours[n]
                );
            }
        }
        assert_eq!(qs[k], qt[k], "Quickshell's and qml6's at scale {scale}");
        if breeze {
            assert_eq!(qt[k][0], BREEZE[k], "Qt's `bz` at scale {scale}");
        }
        for n in 0..icons {
            if ours[n] != qt[k][n] {
                wrong.push(format!(
                    "scale {scale}: {}: drawn from {}, Qt draws it from {}",
                    name(n),
                    ours[n],
                    qt[k][n]
                ));
            }
        }
    }
    (wrong, SCALES.len() * icons)
}

fn check(wrong: Vec<String>) {
    check_of((wrong, SCALES.len() * NAMES.len()));
}

fn check_of((wrong, all): (Vec<String>, usize)) {
    assert!(
        wrong.is_empty(),
        "{} of {all} differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

const NO_CACHE_TEST: &str = concat!(
    module_path!(),
    "::icons_come_from_the_directory_qt_takes_them_from"
);

#[test]
fn icons_come_from_the_directory_qt_takes_them_from() {
    if setup(NO_CACHE_TEST, Cache::None, Home::None) {
        check(differences(true, None));
    }
}

const CACHE_TEST: &str = concat!(
    module_path!(),
    "::a_valid_icon_theme_cache_orders_the_directories"
);

/// With a valid cache the directories go in the cache's order, and `late`, which `u`'s cache
/// does not have, is `p`'s.
#[test]
fn a_valid_icon_theme_cache_orders_the_directories() {
    if setup(CACHE_TEST, Cache::Valid, Home::None) {
        check(differences(false, None));
    }
}

const KICON_TEST: &str = concat!(
    module_path!(),
    "::with_plasma_integration_icons_come_from_the_directory_kiconloader_takes_them_from"
);

/// plasma-integration's KIconEngine asks KIconLoader, which does not read the cache: a
/// directory of the window's scale rounded up that matches the size, else the closest one of
/// scale 1 to the size times the scale (KIconTheme::iconPath).
#[test]
fn with_plasma_integration_icons_come_from_the_directory_kiconloader_takes_them_from() {
    if setup_under(KICON_TEST, Cache::Valid, Home::None, true) {
        check(differences(false, None));
    }
}

const STALE_TEST: &str = concat!(
    module_path!(),
    "::an_icon_theme_cache_older_than_its_theme_is_not_read"
);

#[test]
fn an_icon_theme_cache_older_than_its_theme_is_not_read() {
    if setup(STALE_TEST, Cache::Stale, Home::None) {
        check(differences(true, None));
    }
}

const STALE_DIR_TEST: &str = concat!(
    module_path!(),
    "::an_icon_theme_cache_older_than_a_directory_it_lists_is_not_read"
);

#[test]
fn an_icon_theme_cache_older_than_a_directory_it_lists_is_not_read() {
    if setup(STALE_DIR_TEST, Cache::StaleDir, Home::None) {
        check(differences(true, None));
    }
}

const TRUNCATED_TEST: &str = concat!(module_path!(), "::a_truncated_icon_theme_cache_is_not_read");

#[test]
fn a_truncated_icon_theme_cache_is_not_read() {
    if setup(TRUNCATED_TEST, Cache::Truncated, Home::None) {
        check(differences(true, None));
    }
}

const HASH_OFFSET_TEST: &str = concat!(
    module_path!(),
    "::an_icon_theme_cache_with_a_damaged_header_is_not_read"
);

#[test]
fn an_icon_theme_cache_with_a_damaged_header_is_not_read() {
    if setup(HASH_OFFSET_TEST, Cache::HashOffset, Home::None) {
        check(differences(true, None));
    }
}

const BUCKET_TEST: &str = concat!(
    module_path!(),
    "::an_icon_whose_entry_of_the_cache_is_damaged_is_not_in_the_theme"
);

#[test]
fn an_icon_whose_entry_of_the_cache_is_damaged_is_not_in_the_theme() {
    if setup(BUCKET_TEST, Cache::Bucket, Home::None) {
        check(differences(false, None));
    }
}

const HOME_FILES_TEST: &str = concat!(
    module_path!(),
    "::a_theme_in_two_places_is_searched_in_both"
);

/// The PNG entries of a later place go before the ones of an earlier place, the SVG ones
/// after them.
#[test]
fn a_theme_in_two_places_is_searched_in_both() {
    if setup(HOME_FILES_TEST, Cache::None, Home::Files) {
        check(differences(false, None));
    }
}

const HOME_INDEX_TEST: &str = concat!(
    module_path!(),
    "::the_first_index_theme_names_the_directories_of_every_place"
);

/// Each place has its own cache.
#[test]
fn the_first_index_theme_names_the_directories_of_every_place() {
    if setup(HOME_INDEX_TEST, Cache::Valid, Home::Index) {
        check(differences(false, None));
    }
}

const DOT_ICONS_TEST: &str = concat!(module_path!(), "::a_theme_in_dot_icons_is_searched_first");

/// `~/.icons` goes before `$XDG_DATA_HOME/icons`: its SVG is the one drawn, and its PNG of
/// an icon the other places have only in the inherited theme.
#[test]
fn a_theme_in_dot_icons_is_searched_first() {
    if setup(DOT_ICONS_TEST, Cache::Valid, Home::FilesAndDotIcons) {
        check(differences(false, None));
    }
}

const NAME_AT_END_TEST: &str = concat!(
    module_path!(),
    "::an_icon_theme_cache_with_a_directory_named_at_its_end_is_not_read"
);

#[test]
fn an_icon_theme_cache_with_a_directory_named_at_its_end_is_not_read() {
    if setup(NAME_AT_END_TEST, Cache::NameAtEnd, Home::None) {
        check(differences(true, None));
    }
}

const IMAGES_PAST_END_TEST: &str = concat!(
    module_path!(),
    "::an_icon_theme_cache_is_not_read_after_an_icon_whose_images_end_past_its_end"
);

/// `near` is the first icon asked for, so no icon is taken from the cache.
#[test]
fn an_icon_theme_cache_is_not_read_after_an_icon_whose_images_end_past_its_end() {
    if setup(IMAGES_PAST_END_TEST, Cache::ImagesPastEnd, Home::None) {
        check(differences(true, Some("near")));
    }
}

const ODD_BUCKET_TEST: &str = concat!(
    module_path!(),
    "::icons_asked_before_the_cache_is_found_damaged_are_the_ones_qt_asks_before_it"
);

/// Qt asks for the icons of a window from the last one declared to the first
/// (`QQuickWindowPrivate::polishItems`), and so does the shell: both take `late`, `hi2`,
/// `hi`, `leg`, `nolist` and `thr` from the cache and drop it at `near`.
#[test]
fn icons_asked_before_the_cache_is_found_damaged_are_the_ones_qt_asks_before_it() {
    if setup(ODD_BUCKET_TEST, Cache::OddBucket, Home::None) {
        check(differences(false, None));
    }
}

const SCALE_CHANGES_TEST: &str = concat!(
    module_path!(),
    "::an_icon_asked_again_at_another_scale_comes_from_the_directories_found_for_it_first"
);

/// QIcon::fromTheme keeps the icon it made for a name, with the directories found for it
/// then: one process of Qt whose output changes its scale takes `late` from the cache's
/// directory at every scale, though the cache was dropped at `near` at the first.
#[test]
fn an_icon_asked_again_at_another_scale_comes_from_the_directories_found_for_it_first() {
    if setup(SCALE_CHANGES_TEST, Cache::OddBucket, Home::None) {
        check_of(differences_of(
            &scene_qml(None),
            NAMES.len(),
            Process::AllScales,
            false,
        ));
    }
}

const SIZE_AGAIN_TEST: &str = concat!(
    module_path!(),
    "::an_icon_asked_again_at_another_size_comes_from_the_directories_found_for_it_first"
);

/// The same in one window at one scale: `late` is asked first at 26 and, after `near`
/// dropped the cache, at 16.
#[test]
fn an_icon_asked_again_at_another_size_comes_from_the_directories_found_for_it_first() {
    if setup(SIZE_AGAIN_TEST, Cache::OddBucket, Home::None) {
        check_of(differences_of(
            &scene_again_qml("late"),
            NAMES.len() + 1,
            Process::EachScale,
            false,
        ));
    }
}
