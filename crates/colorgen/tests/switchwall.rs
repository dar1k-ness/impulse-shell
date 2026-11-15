//! Equivalence with the original `switchwall.sh` + `applycolor.sh` + `material-code-set-color.sh`:
//! `tests/switchwall/gen.sh` ran them (real matugen 4.2.0 and materialyoucolor 2.0.10) in a
//! sandbox HOME with logging fakes; this replays every case through the Rust port.

use colorgen::switchwall::{Accent, Io, Paths, Request, is_hex, main, run, wait_background};
use colorgen::sys::Sys;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/switchwall");
const II: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml");
const OUTPUT_DIRS: [&str; 10] = [
    ".local",
    ".cache",
    ".config/hypr",
    ".config/fuzzel",
    ".config/gtk-3.0",
    ".config/gtk-4.0",
    ".config/impulse-shell",
    ".config/Code",
    ".config/Cursor",
    "pts",
];

struct Fake {
    home: PathBuf,
    log: Mutex<Vec<String>>,
    /// Record config edits instead of writing the file (the shell's Config route).
    edits: Option<Mutex<Vec<String>>>,
}

impl Fake {
    fn log(&self, s: String) {
        self.log
            .lock()
            .unwrap()
            .push(s.replace(self.home.to_str().unwrap(), "@HOME@"));
    }
}

fn name(a: &str) -> &str {
    a.rsplit('/').next().unwrap()
}

impl Sys for Fake {
    fn hypr_json(&self, cmd: &str) -> Option<serde_json::Value> {
        assert_eq!(cmd, "monitors");
        serde_json::from_str(r#"[{"name":"DP-1","width":1920,"height":1080,"x":0,"y":0,"scale":1.0,"focused":true},{"name":"HDMI-A-1","width":1280,"height":720,"x":1920,"y":0,"scale":1.0,"focused":false}]"#).ok()
    }
    fn gsettings_get(&self, _key: &str) -> Option<String> {
        Some("prefer-dark".into())
    }
    fn gsettings_set(&self, key: &str, value: &str) {
        self.log(format!(
            "gsettings set org.gnome.desktop.interface {key} {value}"
        ));
    }
    fn run(&self, argv: &[String], _cwd: Option<&Path>) -> Option<String> {
        self.log(format!("{} {}", name(&argv[0]), argv[1..].join(" ")));
        match name(&argv[0]) {
            "hyprpicker" => Some("#3A5F0B\n".into()),
            "ffmpeg" => {
                std::fs::copy(format!("{DIR}/fixture/thumb.jpg"), argv.last().unwrap()).ok()?;
                Some(String::new())
            }
            _ => Some(String::new()),
        }
    }
    fn spawn(&self, argv: &[String]) {
        self.log(format!("{} {}", name(&argv[0]), argv[1..].join(" ")));
    }
    fn kde_recolor(
        &self,
        dirs: &colorgen::kde::Dirs,
        seed: &str,
        dark: bool,
        sv: usize,
    ) -> colorgen::kde::Outcome {
        // The original runs the wrapper (which reads the mode and color.txt itself).
        assert!(dark, "fake gsettings says prefer-dark");
        assert_eq!(dirs.config, self.home.join(".config"));
        assert!(seed.starts_with('#') && seed.len() == 7, "{seed}");
        const VARIANTS: [&str; 9] = [
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
        self.log(format!(
            "kde-material-you-colors-wrapper.sh --scheme-variant {}",
            VARIANTS[sv]
        ));
        colorgen::kde::Outcome::Applied(Default::default())
    }
    fn has_command(&self, _name: &str) -> bool {
        true
    }
    fn kill_exact_wait(&self, name: &str, sig: i32) {
        self.log(format!("pkill -{sig} -x {name}; pidwait -x {name}"));
    }
    fn signal_named(&self, name: &str, _sig: i32) {
        self.log(format!("pgrep -f {name}"));
    }
    fn ttys(&self) -> Vec<PathBuf> {
        vec![self.home.join("pts/0"), self.home.join("pts/1")]
    }
    fn write_tty(&self, tty: &Path, data: &[u8]) {
        std::fs::write(tty, data).unwrap();
    }
    fn notify(
        &self,
        app: &str,
        category: Option<&str>,
        summary: &str,
        body: &str,
        actions: &[(&str, &str)],
        then: colorgen::sys::OnAction,
    ) {
        let acts: String = actions
            .iter()
            .map(|(k, l)| format!(" -A {k}={l}"))
            .collect();
        self.log(match category {
            None => format!("notify-send {summary} {body}{acts} -a {app}"),
            Some(c) => format!("notify-send -a {app} -c {c}{acts} {summary} {body}"),
        });
        then(None);
    }
    fn set_config(&self, file: &Path, path: &[&str], value: &str) {
        match &self.edits {
            Some(e) => e
                .lock()
                .unwrap()
                .push(format!("{}={value}", path.join("."))),
            None => colorgen::switchwall::jq_set(file, path, value),
        }
    }
}

fn copy_tree(from: &Path, to: &Path, map: &dyn Fn(Vec<u8>) -> Vec<u8>) {
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let dst = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&dst).unwrap();
            copy_tree(&e.path(), &dst, map);
        } else {
            std::fs::create_dir_all(to).unwrap();
            std::fs::write(&dst, map(std::fs::read(e.path()).unwrap())).unwrap();
            let perm = std::fs::metadata(e.path()).unwrap().permissions();
            std::fs::set_permissions(&dst, perm).unwrap();
        }
    }
}

fn files(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root.join(rel)) else {
        return;
    };
    for e in rd.flatten() {
        let r = rel.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            files(root, &r, out);
        } else {
            out.push(r);
        }
    }
}

fn snapshot(root: &Path, home: Option<&str>) -> Vec<(PathBuf, String)> {
    let mut v = Vec::new();
    for d in OUTPUT_DIRS {
        files(root, Path::new(d), &mut v);
    }
    v.sort();
    v.into_iter()
        .map(|f| {
            let t = String::from_utf8_lossy(&std::fs::read(root.join(&f)).unwrap()).into_owned();
            let t = home.map_or(t.clone(), |h| t.replace(h, "@HOME@"));
            let t = t
                .lines()
                .map(|l| {
                    if l.starts_with("# Time: ") {
                        "# Time: @DATE@"
                    } else {
                        l
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + if t.ends_with('\n') { "\n" } else { "" };
            (f, t)
        })
        .collect()
}

fn prepare(case: &Path) -> (PathBuf, String, Vec<String>, Paths) {
    let cname = case.file_name().unwrap().to_str().unwrap();
    // A home of its own for every run: tests run the same case at the same time.
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let home = std::env::temp_dir().join(format!("colorgen-sw-{}-{n}-{cname}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let h = home.to_str().unwrap().to_string();
    let ident = |b: Vec<u8>| b;
    copy_tree(&Path::new(DIR).join("fixture/home"), &home, &ident);
    let script_dir = home.join(".config/rustshell/impulse-shell/scripts/colors");
    copy_tree(
        &Path::new(II).join("scripts/colors/terminal"),
        &script_dir.join("terminal"),
        &ident,
    );
    copy_tree(
        &Path::new(II).join("assets/images"),
        &home.join(".config/rustshell/impulse-shell/assets/images"),
        &ident,
    );
    std::fs::create_dir_all(home.join("pts")).unwrap();
    for t in ["0", "1", "ptmx"] {
        std::fs::write(home.join("pts").join(t), "").unwrap();
    }
    let unnorm = |b: Vec<u8>| {
        String::from_utf8(b)
            .unwrap()
            .replace("@HOME@", &h)
            .into_bytes()
    };
    if case.join("pre").is_dir() {
        copy_tree(&case.join("pre"), &home, &unnorm);
    }
    let wall = home.join("Pictures/Wallpapers");
    let args: Vec<String> = std::fs::read_to_string(case.join("args"))
        .unwrap()
        .lines()
        .map(|a| a.replace("@W@", wall.to_str().unwrap()))
        .collect();
    let paths = Paths {
        home: home.clone(),
        config_home: home.join(".config"),
        cache_home: home.join(".cache"),
        state_home: home.join(".local/state"),
        script_dir,
    };
    (home, h, args, paths)
}

/// What the shell asks for where the script was given `args`.
fn request(args: &[String]) -> Request {
    let mut r = Request::default();
    let mut args = args.iter().peekable();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--mode" => r.mode = args.next().unwrap().clone(),
            "--type" => r.scheme = args.next().unwrap().clone(),
            "--image" => r.image = args.next().unwrap().clone(),
            "--noswitch" => r.keep_wallpaper = true,
            "--color" => {
                r.accent = Some(match args.next_if(|c| *c == "clear" || is_hex(c)) {
                    Some(c) if c == "clear" => Accent::Clear,
                    Some(c) => Accent::Set(c.clone()),
                    None => Accent::Pick,
                })
            }
            image => r.image = image.to_string(),
        }
    }
    r
}

/// The case run as the script's command line, or as the request the shell makes.
fn run_case(case: &Path, as_request: bool) -> Result<(), String> {
    run_case_with(case, as_request, false)
}

/// `config_saved_later`: config edits are only recorded, as when the shell's Config takes them
/// and saves the file after the run; config.json is then not compared.
fn run_case_with(case: &Path, as_request: bool, config_saved_later: bool) -> Result<(), String> {
    let cname = case.file_name().unwrap().to_str().unwrap();
    let (home, h, args, paths) = prepare(case);
    let fake = Arc::new(Fake {
        home: home.clone(),
        log: Mutex::new(Vec::new()),
        edits: config_saved_later.then(|| Mutex::new(Vec::new())),
    });
    let sys: Arc<dyn Sys> = fake.clone();
    let rc = if as_request {
        run(&request(&args), &paths, &sys, Io::STD)
    } else {
        main(&args, &paths, &sys)
    };
    wait_background();
    let mut errs = Vec::new();
    let want_rc: i32 = std::fs::read_to_string(case.join("rc"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    if rc != want_rc {
        errs.push(format!("rc {rc} != {want_rc}"));
    }
    let mut calls = fake.log.lock().unwrap().clone();
    calls.sort();
    let want_calls: Vec<String> = std::fs::read_to_string(case.join("calls"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    if calls != want_calls {
        errs.push(format!("calls:\n  got  {calls:?}\n  want {want_calls:?}"));
    }
    let got = snapshot(&home, Some(&h));
    let want = snapshot(&case.join("files"), None);
    let names = |v: &[(PathBuf, String)]| v.iter().map(|f| f.0.clone()).collect::<Vec<_>>();
    if names(&got) != names(&want) {
        errs.push(format!(
            "files:\n  got  {:?}\n  want {:?}",
            names(&got),
            names(&want)
        ));
    }
    for (f, t) in &want {
        if config_saved_later && f.ends_with("impulse-shell/config.json") {
            continue;
        }
        if let Some((_, g)) = got.iter().find(|(gf, _)| gf == f)
            && g != t
        {
            let line = g.lines().zip(t.lines()).position(|(a, b)| a != b);
            errs.push(format!("{} differs at line {line:?}", f.display()));
        }
    }
    let _ = std::fs::remove_dir_all(&home);
    if errs.is_empty() {
        Ok(())
    } else {
        Err(format!("{cname}: {}", errs.join("\n")))
    }
}

#[test]
fn switchwall_matches_original() {
    let mut cases: Vec<PathBuf> = std::fs::read_dir(Path::new(DIR).join("expected"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    cases.sort();
    assert!(cases.len() >= 20);
    // Each case twice: as the script's command line, then as the request the shell makes.
    let fails: Vec<String> = [false, true]
        .iter()
        .flat_map(|&as_request| {
            let how = if as_request { "request" } else { "arguments" };
            cases
                .iter()
                .filter_map(move |c| run_case(c, as_request).err())
                .map(move |e| format!("as {how}: {e}"))
        })
        .collect();
    assert!(
        fails.is_empty(),
        "{} of {} cases differ:\n{}",
        fails.len(),
        cases.len(),
        fails.join("\n\n")
    );
}

/// The accent color a run is asked for is the one its colors are made from, also when the
/// shell's Config saves it to config.json only after the run: `/accentcolor #ff0000` left the
/// colors as they were, and the next `/accentcolor clear` made them red.
#[test]
fn accent_asked_for_is_used_before_config_is_saved() {
    for case in ["noswitch_color", "noswitch_color_nohash", "noswitch_clear"] {
        let case = Path::new(DIR).join("expected").join(case);
        for as_request in [false, true] {
            if let Err(e) = run_case_with(&case, as_request, true) {
                panic!("as request {as_request}: {e}");
            }
        }
    }
}

/// A request that keeps the wallpaper and names it goes by that picture, not by the one in
/// config.json: the caller has just set it, and the shell's Config saves it only after the
/// run. The "auto" scheme of a wallpaper put back was the one of the wallpaper it replaced.
#[test]
fn kept_wallpaper_named_by_the_request_is_the_one_used() {
    let scheme_for = |image: &str| {
        let (home, _, _, paths) = prepare(&Path::new(DIR).join("expected/noswitch_color"));
        let wall = |name: &str| format!("{}/Pictures/Wallpapers/{name}", home.display());
        let config = home.join(".config/impulse-shell/config.json");
        colorgen::switchwall::jq_set(
            &config,
            &["background", "wallpaperPath"],
            &wall("syn_gray.png"),
        );
        colorgen::switchwall::jq_set(&config, &["appearance", "palette", "type"], "auto");
        let sys: Arc<dyn Sys> = Arc::new(Fake {
            home: home.clone(),
            log: Mutex::new(Vec::new()),
            edits: Some(Mutex::new(Vec::new())),
        });
        let req = Request {
            image: if image.is_empty() {
                String::new()
            } else {
                wall(image)
            },
            keep_wallpaper: true,
            accent: Some(Accent::Set("#A52F48".into())),
            ..Default::default()
        };
        assert_eq!(run(&req, &paths, &sys, Io::STD), 0);
        wait_background();
        let scheme = home.join(".local/state/rustshell/user/generated/scheme_type.txt");
        let scheme = std::fs::read_to_string(scheme).unwrap();
        let _ = std::fs::remove_dir_all(&home);
        scheme.trim().to_string()
    };
    assert_eq!(scheme_for(""), "scheme-neutral");
    assert_eq!(scheme_for("syn_gray.png"), "scheme-neutral");
    assert_eq!(scheme_for("syn_grad.jpg"), "scheme-tonal-spot");
}

/// switchwall edits the shell config only through `Sys::set_config` (which the shell routes
/// through its Config singleton), never by writing config.json behind Config's back.
#[test]
fn config_edits_go_through_sys() {
    let case = Path::new(DIR).join("expected/img_accent_cleared");
    let (home, _h, args, paths) = prepare(&case);
    let cfg = home.join(".config/impulse-shell/config.json");
    let before = std::fs::read_to_string(&cfg).unwrap();
    let fake = Arc::new(Fake {
        home: home.clone(),
        log: Mutex::new(Vec::new()),
        edits: Some(Mutex::new(Vec::new())),
    });
    let sys: Arc<dyn Sys> = fake.clone();
    assert_eq!(main(&args, &paths, &sys), 0);
    let edits = fake.edits.as_ref().unwrap().lock().unwrap().clone();
    let wall = home.join("Pictures/Wallpapers/syn_small.png");
    assert!(
        edits.contains(&format!("background.wallpaperPath={}", wall.display())),
        "{edits:?}"
    );
    assert!(
        edits.contains(&"appearance.palette.accentColor=".to_string()),
        "{edits:?}"
    );
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), before);
    let _ = std::fs::remove_dir_all(&home);
}

/// Concurrent `jq_set`s on one file (switchwall runs and the shell's fallback) keep every edit.
#[test]
fn concurrent_jq_set_keeps_all_edits() {
    let d = std::env::temp_dir().join(format!("colorgen-jqset-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("config.json");
    std::fs::write(&f, "{}\n").unwrap();
    let ts: Vec<_> = (0..8)
        .map(|t| {
            let f = f.clone();
            std::thread::spawn(move || {
                for i in 0..25 {
                    colorgen::switchwall::jq_set(&f, &[&format!("t{t}"), &format!("k{i}")], "v");
                }
            })
        })
        .collect();
    for t in ts {
        t.join().unwrap();
    }
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    for t in 0..8 {
        for i in 0..25 {
            assert_eq!(v[format!("t{t}")][format!("k{i}")], "v", "t{t}.k{i} lost");
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// The restore script Hyprland runs at login, executed with logging stubs: it kills mpvpaper by
/// exact name, waits for it to exit, and starts one mpvpaper per monitor with the video path
/// passed through literally, without any sleep.
#[test]
fn restore_script_starts_one_mpvpaper_per_monitor() {
    if !testenv::require("bash") || !testenv::require("jq") {
        return;
    }
    let case = Path::new(DIR).join("expected/video");
    let (home, _, mut args, paths) = prepare(&case);
    let odd = home.join("Pictures/Wallpapers/it's $HOME \"x\".mp4");
    std::fs::copy(home.join("Pictures/Wallpapers/clip.mp4"), &odd).unwrap();
    *args.last_mut().unwrap() = odd.to_str().unwrap().into();
    let fake = Arc::new(Fake {
        home: home.clone(),
        log: Mutex::new(Vec::new()),
        edits: None,
    });
    let sys: Arc<dyn Sys> = fake.clone();
    assert_eq!(main(&args, &paths, &sys), 0);
    let script = home.join(".config/hypr/custom/scripts/__restore_video_wallpaper.sh");
    let text = std::fs::read_to_string(&script).unwrap();
    assert!(
        !text.contains("sleep") && !text.contains("pkill -f"),
        "{text}"
    );

    let bin = home.join("stub-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = home.join("restore.log");
    let stub = |name: &str, body: &str| {
        let p = bin.join(name);
        std::fs::write(&p, format!("#!/bin/bash\n{body}\n")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    // One write per record: the mpvpaper starts run concurrently and share the O_APPEND log.
    let record = r#"rec=$(basename "$0"); for a in "$@"; do rec+=$'\t'$a; done; printf '%s\n' "$rec" >> "$LOG""#;
    for n in ["pkill", "pidwait", "mpvpaper"] {
        stub(n, record);
    }
    stub("timeout", &format!("{record}\nshift; exec \"$@\""));
    stub(
        "hyprctl",
        r#"echo '[{"name":"DP-1"},{"name":"HDMI-A-1"},{"name":"eDP-1"}]'"#,
    );
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    // Sourced so `wait` collects the backgrounded mpvpaper starts.
    let st = std::process::Command::new("bash")
        .args(["-c", "source \"$0\"; wait", script.to_str().unwrap()])
        .env("PATH", path)
        .env("LOG", &log)
        .status()
        .unwrap();
    assert!(st.success());
    let lines: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    let v = odd.to_str().unwrap();
    let opts = colorgen::switchwall::VIDEO_OPTS;
    assert_eq!(
        &lines[..3],
        [
            "pkill\t-9\t-x\tmpvpaper",
            "timeout\t2\tpidwait\t-x\tmpvpaper",
            "pidwait\t-x\tmpvpaper"
        ]
    );
    let mut starts = lines[3..].to_vec();
    starts.sort();
    assert_eq!(
        starts,
        ["DP-1", "HDMI-A-1", "eDP-1"].map(|m| format!("mpvpaper\t-o\t{opts}\t{m}\t{v}"))
    );
    let _ = std::fs::remove_dir_all(&home);
}

/// The system refuses the thread `applycolor` runs kitty's part on beside the terminals' (the
/// limit on threads is reached): both parts still run, on the calling thread, instead of a
/// panic. The open terminals get their sequences and kitty is told to reload.
#[test]
fn applycolor_does_both_parts_when_its_thread_is_refused() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let (script_dir, state_dir) = (home.join("scripts"), home.join("state"));
    for d in [
        script_dir.join("terminal"),
        state_dir.join("user/generated/terminal"),
        home.join("pts"),
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(script_dir.join("terminal/sequences.txt"), "fg=$primary #\n").unwrap();
    std::fs::write(
        script_dir.join("terminal/kitty-theme.conf"),
        "color $primary #\n",
    )
    .unwrap();
    std::fs::write(
        state_dir.join("user/generated/material_colors.scss"),
        "$primary: #112233;\n",
    )
    .unwrap();
    let fake = Fake {
        home: home.to_path_buf(),
        log: Mutex::new(Vec::new()),
        edits: None,
    };
    let quiet = |_: String| {};
    let io = colorgen::switchwall::Io {
        out: &quiet,
        err: &quiet,
    };
    testenv::refuse_new_threads();
    let applied = colorgen::apply::applycolor(
        &colorgen::apply::Paths {
            script_dir: &script_dir,
            state_dir: &state_dir,
            qs_config_dir: home,
            shell_config: &home.join("no-config.json"),
        },
        &fake,
        io,
    );
    assert!(applied);
    assert_eq!(*fake.log.lock().unwrap(), ["pgrep -f kitty"]);
    for tty in ["pts/0", "pts/1"] {
        let got = std::fs::read_to_string(home.join(tty)).unwrap();
        assert!(got.contains("112233"), "{tty}: {got:?}");
    }
}
