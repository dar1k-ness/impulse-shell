//! The names local time is printed under (`t` and `tttt` of a date format) are Qt 6.12's in
//! every state of what names the system's zone: /etc/localtime as a link, a chain of links or
//! a file, /etc/TZ, /etc/timezone, `TZ` and `TZDIR`. zone_names_qt/steps.txt has the states,
//! show.qml what is asked in each, and answers.qt what Qt answered (zone_names_qt/qt.sh
//! writes it, in the tools/testenv image). The steps run here as they ran there: over an
//! /etc of the test's own, one process a group of steps.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use std::path::Path;
use std::process::Command;

const TEST: &str = concat!(module_path!(), "::local_time_is_named_as_qt_names_it");
/// The group of steps this process runs; without it, the process that runs them all.
const GROUP: &str = "ZONE_NAMES_GROUP";
/// Where a group's process leaves its answers.
const OUT: &str = "/etc/answers";
const STEPS: &str = include_str!("zone_names_qt/steps.txt");
const SHOW: &str = include_str!("zone_names_qt/show.qml");
const ANSWERS: &str = include_str!("zone_names_qt/answers.qt");
const LOCAL: &str = "/etc/localtime";

#[test]
fn local_time_is_named_as_qt_names_it() {
    if !Path::new("/usr/share/zoneinfo/zone.tab").exists() {
        testenv::equiv_missing("the time zone database in /usr/share/zoneinfo");
        return;
    }
    if !testenv::in_own_process(TEST) {
        testenv::run_in_own_process_with_own_etc(TEST, [("LC_ALL", "C.UTF-8")]);
    } else if let Ok(group) = std::env::var(GROUP) {
        let answers = show(group.parse().expect("a group's number"));
        std::fs::write(OUT, answers).expect("write the answers");
    } else {
        compare();
    }
}

/// Runs every group of steps in a process of its own and compares what they answer with
/// Qt's, line by line.
fn compare() {
    // What qt.sh sets up: a copy of the database, with a zone CLDR has no name for, and the
    // database under a path with two `zoneinfo` in it.
    let sh = "cp -rL /usr/share/zoneinfo /etc/tzcopy && mkdir -p /etc/tzcopy/Test \
              /etc/zoneinfo/usr/share && cp /etc/tzcopy/Asia/Tokyo /etc/tzcopy/Test/Zone && \
              ln -s /usr/share/zoneinfo /etc/zoneinfo/usr/share/zoneinfo";
    let done = Command::new("sh").args(["-c", sh]).status();
    assert!(done.is_ok_and(|s| s.success()), "{sh}");

    let groups = STEPS.lines().filter(|l| l.starts_with("fresh ")).count();
    let mut ours = String::new();
    for group in 0..groups {
        for entry in std::fs::read_dir("/etc").expect("/etc").flatten() {
            if entry.file_type().is_ok_and(|t| !t.is_dir()) {
                std::fs::remove_file(entry.path()).expect("remove a step's file");
            }
        }
        let mut process = testenv::own_process_command(TEST);
        for unset in ["TZ", "TZDIR", "LANG", "LANGUAGE", "LC_TIME", "LC_NUMERIC"] {
            process.env_remove(unset);
        }
        let ran = process.env(GROUP, group.to_string()).output().expect("run");
        let stdout = String::from_utf8_lossy(&ran.stdout);
        assert!(ran.status.success(), "group {group}:\n{stdout}");
        ours += &std::fs::read_to_string(OUT).expect("a group's answers");
    }

    let (qt, ours): (Vec<_>, Vec<_>) = (ANSWERS.lines().collect(), ours.lines().collect());
    let differ: Vec<_> = qt
        .iter()
        .zip(&ours)
        .filter(|(qt, ours)| qt != ours)
        .collect();
    for (qt, ours) in &differ {
        println!("Qt:   {qt}\nours: {ours}");
    }
    println!("{} compared with Qt, {} differ", qt.len(), differ.len());
    assert_eq!(qt.len(), ours.len(), "answers");
    assert!(differ.is_empty(), "{} of {} differ", differ.len(), qt.len());
}

/// Runs the steps of `group` (the lines after its `fresh`) as zone_names_qt/qt.py does and
/// gives a line for each `show`: the group's title, the step's own and show.qml's answer.
fn show(group: usize) -> String {
    let unlink = || {
        for path in [LOCAL, "/etc/lt2"] {
            let _ = std::fs::remove_file(path);
        }
    };
    let link = |to: &str, at: &str| std::os::unix::fs::symlink(to, at).expect("link");
    let write = |path: &str, text: &str| std::fs::write(path, format!("{text}\n")).expect("write");
    #[expect(
        clippy::disallowed_methods,
        reason = "the system's zone changes under a running process"
    )]
    let set = |name: &str, value: Option<&str>| {
        match value {
            // SAFETY: this process runs this test only, and nothing else reads its
            // environment meanwhile.
            Some(value) => unsafe { std::env::set_var(name, value) },
            // SAFETY: as above.
            None => unsafe { std::env::remove_var(name) },
        }
    };

    let (mut n, mut title, mut answers) = (None, "", String::new());
    let mut document = None;
    for line in STEPS.lines() {
        let (op, arg) = line.split_once(' ').unwrap_or((line, ""));
        if op == "fresh" {
            (n, title) = (Some(n.map_or(0, |n| n + 1)), arg);
            continue;
        }
        if n != Some(group) {
            continue;
        }
        match op {
            "link" => {
                unlink();
                link(arg, LOCAL);
            }
            "link2" => {
                unlink();
                link(arg, "/etc/lt2");
                link("/etc/lt2", LOCAL);
            }
            "link2rel" => {
                unlink();
                link(arg, "/etc/lt2");
                link("lt2", LOCAL);
            }
            "chain" => {
                let (count, target) = arg.split_once(' ').expect("a count and a target");
                let count: usize = count.parse().expect("a count");
                unlink();
                for i in 1..count {
                    let next = format!("/etc/c{}", i + 1);
                    link(
                        if i == count - 1 { target } else { &next },
                        &format!("/etc/c{i}"),
                    );
                }
                link(if count > 1 { "/etc/c1" } else { target }, LOCAL);
            }
            "tz" => set("TZ", Some(arg)),
            "untz" => set("TZ", None),
            "tzdir" => set("TZDIR", Some(arg)),
            "copy" => {
                unlink();
                std::fs::copy(arg, LOCAL).expect("copy");
            }
            "timezone" => write("/etc/timezone", arg),
            "retimezone" => {
                write("/etc/timezone.new", arg);
                std::fs::rename("/etc/timezone.new", "/etc/timezone").expect("rename");
            }
            "etctz" => write("/etc/TZ", arg),
            "show" => {
                let (rt, root) = document.get_or_insert_with(|| {
                    let mut rt = Runtime::new(None);
                    let root = rt.load_str(SHOW).expect("show.qml");
                    rt.settle();
                    (rt, root)
                });
                let text = rt.call_method(*root, "show", vec![]);
                let text = text.expect("show()").expect("show() returns");
                answers += &format!("{title}\t{arg}\t{}\n", text.to_js_string());
            }
            _ => panic!("steps.txt: {line}"),
        }
    }
    answers
}
