//! The links that mean "this project" open the impulse-shell repository: "Source" and "Report
//! issue" on the settings About page, "GitHub" in the welcome window. No link of the tree sends
//! a bug report or a question to another project's tracker.
use qml::runtime::Runtime;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "https://github.com/dar1k-ness/impulse-shell";

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            sources(&p, out);
        } else if matches!(
            p.extension().and_then(|x| x.to_str()),
            Some("qml" | "js" | "mjs")
        ) {
            out.push(p);
        }
    }
}

#[test]
fn no_link_to_another_projects_tracker() {
    let mut files = vec![];
    sources(&ri(), &mut files);
    assert!(files.len() > 100, "{} files under qml", files.len());
    let mut hits = vec![];
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for (n, line) in text.lines().enumerate() {
            // end-4's repository (its issues and discussions), and this project's old name.
            if line.contains("github.com/end-4/dots-hyprland") || line.contains("end-4-golang") {
                hits.push(format!("{}:{}: {}", f.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(hits.is_empty(), "{hits:#?}");
}

fn load(file: &str) -> Runtime {
    let r = ri();
    let mut rt = Runtime::new(Some(r.clone()));
    let dirs: Vec<PathBuf> = [
        "modules/common",
        "modules/common/functions",
        "modules/common/utils",
        "services",
        "",
    ]
    .iter()
    .map(|d| r.join(d))
    .collect();
    rt.load_singletons_lazy(&dirs);
    rt.load_file(&r.join(file))
        .unwrap_or_else(|e| panic!("{e}"));
    // The settings pages are built after the window, on the frames that follow.
    let mut now = 0f64;
    for _ in 0..200 {
        rt.poll_native();
        now += 8.0;
        rt.tick(now);
    }
    rt
}

/// The one button labelled `text`.
fn button(rt: &Runtime, text: &str) -> usize {
    let n = rt.world.borrow().items.len();
    let hits: Vec<usize> = (0..n)
        .filter(|&i| {
            let w = rt.world.borrow();
            w.items.get(i).is_some() && !w.dead.contains(&i)
        })
        .filter(|&i| rt.has_handler(i, "onClicked") && rt.get(i, "mainText").to_js_string() == text)
        .collect();
    assert_eq!(hits.len(), 1, "buttons labelled {text:?}: {hits:?}");
    hits[0]
}

#[test]
fn project_links_open_impulse_shell() {
    const TEST: &str = concat!(module_path!(), "::project_links_open_impulse_shell");
    // An `xdg-open` first on PATH that reports its argument through a FIFO, no session bus (so
    // no portal takes the URL), and a home of its own, in a process that has them in its
    // environment.
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let (bin, home) = (d.join("bin"), d.join("home"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(home.join(".config/impulse-shell")).unwrap();
        std::fs::write(home.join(".config/impulse-shell/config.json"), "{}\n").unwrap();
        let calls = d.join("xdg-open.calls");
        assert!(
            Command::new("mkfifo")
                .arg(&calls)
                .status()
                .unwrap()
                .success()
        );
        testenv::install_script(
            &bin.join("xdg-open"),
            &format!("#!/bin/sh\necho \"$@\" > '{}'\n", calls.display()),
        );
        let path = std::env::var("PATH").unwrap_or_default();
        let mut vars: Vec<(&str, std::ffi::OsString)> = testenv::home_vars(&home);
        vars.push(("PATH", format!("{}:{path}", bin.display()).into()));
        vars.push((
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", d.join("no-bus").display()).into(),
        ));
        vars.push(("LINKS_CALLS", calls.into()));
        testenv::run_in_own_process(TEST, vars);
        return;
    }
    // Open for writing too, so the FIFO never reads end-of-file between calls: each read
    // blocks until the next `xdg-open` writes its line.
    let mut calls = BufReader::new(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(std::env::var_os("LINKS_CALLS").unwrap())
            .unwrap(),
    );
    let mut opened = |rt: &mut Runtime, text: &str| {
        let b = button(rt, text);
        rt.emit(b, "clicked", &[], vec![]);
        rt.settle();
        let mut line = String::new();
        calls.read_line(&mut line).unwrap();
        line.trim_end().to_string()
    };

    let mut rt = load("settings.qml");
    assert_eq!(opened(&mut rt, "Source"), REPO);
    assert_eq!(opened(&mut rt, "Report issue"), format!("{REPO}/issues"));
    drop(rt);

    let mut rt = load("welcome.qml");
    assert_eq!(opened(&mut rt, "GitHub"), REPO);
    // The documentation is the original project's, and the buttons say so.
    for (text, url) in [
        (
            "Usage (ii wiki)",
            "https://end-4.github.io/dots-hyprland-wiki/en/ii-qs/02usage/",
        ),
        (
            "Configuration (ii wiki)",
            "https://end-4.github.io/dots-hyprland-wiki/en/ii-qs/03config/",
        ),
    ] {
        assert_eq!(opened(&mut rt, text), url);
    }
}
