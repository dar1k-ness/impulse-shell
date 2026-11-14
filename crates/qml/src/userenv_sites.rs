//! The programs the shell's own code starts, each of which the original started with a `Process` or
//! `Rustshell.execDetached` (directly or from a script one of them ran): every one gets the
//! user's environment ([`workpool::userenv`]), not this process's with the config's pragmas.
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A program that appends what it sees of `RI_SITE_MARK` to `$RI_SITE_OUT/<its name>`.
/// `slurp` also prints a region, which the recorder goes on with.
const PROGRAM: &str = r#"#!/bin/sh
name=${0##*/}
echo "${RI_SITE_MARK-unset}" >> "$RI_SITE_OUT/$name"
[ "$name" = slurp ] && echo "0,0 10x10"
exit 0
"#;

/// The names in `PATH`.
const NAMES: [&str; 14] = [
    "easyeffects",
    "easyeffects-probe",
    "pacman-conf",
    "warp-cli",
    "hyprsunset",
    "slurp",
    "wf-recorder",
    "tesseract",
    "xdg-open",
    "swappy",
    "songrec",
    "cliphist",
    "ri-paste-key",
    "kdialog",
];

/// What the `count` runs of `name` saw, one per line.
fn seen(out: &Path, name: &str, count: usize) -> String {
    let f = out.join(name);
    let lines = || std::fs::read_to_string(&f).unwrap_or_default();
    crate::testenv::DirWatch::new(out).until(|| lines().lines().count() >= count);
    lines().lines().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_shell_s_own_programs_get_the_user_s_environment() {
    const TEST: &str = concat!(
        module_path!(),
        "::the_shell_s_own_programs_get_the_user_s_environment"
    );
    if !crate::testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let (bin, out) = (dir.path().join("bin"), dir.path().join("out"));
        std::fs::create_dir(&bin).unwrap();
        std::fs::create_dir(&out).unwrap();
        for name in NAMES.iter().chain(&["ri-detached", "LaTeX"]) {
            crate::testenv::install_script(&bin.join(name), PROGRAM);
        }
        let mut path = bin.clone().into_os_string();
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap());
        // This process's environment stands for the shell's own, pragmas applied. It names
        // no compositor: the OCR crop copies what tesseract read, and the one the tests
        // share must not get a selection from here.
        crate::testenv::run_in_own_process_env(
            TEST,
            [
                ("PATH", path.as_os_str()),
                ("HOME", dir.path().as_os_str()),
                ("RI_SITE_BIN", bin.as_os_str()),
                ("RI_SITE_OUT", out.as_os_str()),
                ("RI_SITE_MARK", "shell".as_ref()),
            ],
            &["WAYLAND_DISPLAY", "WAYLAND_SOCKET"],
        );
        return;
    }
    let mut user: Vec<_> = std::env::vars_os()
        .filter(|(k, _)| k != "RI_SITE_MARK")
        .collect();
    user.push(("RI_SITE_MARK".into(), "user".into()));
    workpool::userenv::capture_as(user);

    let bin = PathBuf::from(std::env::var_os("RI_SITE_BIN").unwrap());
    let out = PathBuf::from(std::env::var_os("RI_SITE_OUT").unwrap());
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let stop: Arc<crate::svc::Stop> = Default::default();
    let io = crate::ports::Io::detached();
    let dir = bin.to_str().unwrap();

    let _ = crate::easyeffects::program(&["easyeffects", "-x"])
        .unwrap()
        .status();
    let _ = crate::easyeffects::command(&["easyeffects-probe"])
        .unwrap()
        .status();
    crate::latex::run(dir, "LaTeX", &[], &stop);
    crate::jobs_updates::run("pacman-conf", &["DBPath"], bin.as_os_str(), &stop);
    crate::warp::cli(&["status"], &stop);
    crate::warp::start_disconnect();
    crate::probe::sunset::start(&bin.join("hyprsunset"));
    let region = crate::ports::record::Args {
        silent: true,
        ..Default::default()
    };
    crate::ports::record::run(&region, &io);
    // The temporary screenshot a crop reads.
    let shot = |name: &str, after| {
        let p = home.join(name);
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([9, 9, 9, 255]));
        img.save_with_format(&p, image::ImageFormat::Png).unwrap();
        let job = crate::ports::shot::Job::Crop {
            path: p.to_str().unwrap().to_owned(),
            rect: (0, 0, 2, 2),
            after,
        };
        job.run(&io);
    };
    shot("ocr", crate::ports::shot::After::Ocr);
    shot("edit", crate::ports::shot::After::Edit("swappy".into()));
    let endpoint =
        crate::ports::shot::upload_tests::serve(200, r#"{"files":[{"url":"https://host/x.png"}]}"#);
    shot(
        "search",
        crate::ports::shot::After::Search {
            endpoint,
            base: "https://lens.example/?url=".into(),
        },
    );
    let _ = crate::ports::songrec::listen("dev", "1").status();
    let _ = crate::ports::clip::cmd("cliphist list").unwrap().status();
    crate::ports::clip::press_paste("ri-paste-key", &stop);
    let pics = home.join("Pictures");
    std::fs::create_dir(&pics).unwrap();
    let (tx, rx) = crate::svc::Waker::default()
        .handle(crate::svc::Svc::Process)
        .channel();
    crate::jobs::start(
        crate::jobs_wall_gcloud_trans::wall::picker(pics, "kdialog".into()),
        tx,
        stop.clone(),
    );
    assert!(
        rx.into_iter()
            .any(|ev| matches!(ev, crate::native::ProcEv::Exit(_)))
    );
    // `hyprpicker`, the sound player, a desktop entry's program and whatever else starts
    // as `execDetached` did.
    crate::native::detach_program(&["ri-detached".into()]);

    // The program and how many times it ran.
    let sites = [
        ("easyeffects", 1),
        ("easyeffects-probe", 1),
        ("LaTeX", 1),
        ("pacman-conf", 1),
        ("warp-cli", 2),
        ("hyprsunset", 1),
        ("slurp", 1),
        ("wf-recorder", 1),
        ("tesseract", 2),
        ("swappy", 1),
        ("xdg-open", 1),
        ("songrec", 1),
        ("cliphist", 1),
        ("ri-paste-key", 1),
        ("kdialog", 1),
        ("ri-detached", 1),
    ];
    let got: Vec<String> = sites
        .iter()
        .map(|(name, count)| format!("{name}: {}", seen(&out, name, *count)))
        .collect();
    let want: Vec<String> = sites
        .iter()
        .map(|(name, count)| format!("{name}: {}", vec!["user"; *count].join(" ")))
        .collect();
    assert_eq!(got, want, "what each program saw of RI_SITE_MARK");
    // The compositor the tests share (ports_ab's clip tests compare its clipboard) got no
    // selection from here.
    assert_eq!(
        crate::ports::clipsrc::sync(&crate::svc::Stop::default()),
        None,
        "this test set the clipboard of the compositor the other tests share"
    );
}
