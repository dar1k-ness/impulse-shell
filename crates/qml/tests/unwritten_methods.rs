//! A method Qt declares in its `.qmltypes` and this engine has not written must not look like
//! a call that worked: `typeof` says `function` as in Qt, and the call either gives what Qt
//! gives or throws an error that names the method (`docs/qt-differences.md`). The fixture is
//! run in Qt 6.12 and here.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

/// What the error of a declared method that is not written ends with.
const NOT_WRITTEN: &str = "() is declared by Qt and not written in this engine";

/// `label: typeof T, returns V` or `label: typeof T, throws MESSAGE`, as (label, typeof, rest).
fn parts(line: &str) -> (&str, &str, &str) {
    let (label, rest) = line.split_once(": typeof ").expect(line);
    let (ty, rest) = rest.split_once(", ").expect(line);
    (label, ty, rest)
}

/// The lines of a fixture: a message of several lines (the candidates of an overload error)
/// is one entry.
fn entries(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        match out.last_mut() {
            Some(last) if !l.contains(": typeof ") => *last += &format!("\n{l}"),
            _ => out.push(l.clone()),
        }
    }
    out
}

/// Every probed method is what `theirs` has, or says it is not written.
fn compare(with: &str, theirs: &[String], ours: &[String]) {
    let (qt, ours) = (entries(theirs), entries(ours));
    assert_eq!(qt.len(), ours.len(), "{with}: {qt:#?}\nours: {ours:#?}");
    let (mut same, mut thrown, mut silent) = (0, 0, Vec::new());
    for (qt, ours) in qt.iter().zip(&ours) {
        let ((label, qt_type, _), (our_label, our_type, our_rest)) = (parts(qt), parts(ours));
        let method = label.rsplit('.').next().unwrap();
        let method = method.split(' ').next().unwrap();
        if qt == ours {
            same += 1;
        } else if (label, qt_type) == (our_label, our_type)
            && our_rest.starts_with("throws ")
            && our_rest.ends_with(&format!(".{method}{NOT_WRITTEN}"))
        {
            thrown += 1;
        } else {
            silent.push(format!("  {with}: {qt}\n  ours: {ours}"));
        }
    }
    println!(
        "compared with {with}: {} calls, {same} the same, {thrown} say they are not written, \
         {} differ silently",
        qt.len(),
        silent.len()
    );
    assert!(
        silent.is_empty(),
        "differ from {with} without saying so:\n{}",
        silent.join("\n")
    );
}

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/unwritten")
        .join(name)
}

#[test]
fn a_declared_method_is_what_qt_has_or_says_it_is_not_written() {
    if !qt_log::qt_available() {
        return;
    }
    let file = fixture("decl.qml");
    compare("Qt 6.12", &qt_log::qt_log(&file), &qt_log::our_log(&file));
}

/// The methods a call in `qml/` may reach on a value whose type only a run shows
/// (`docs/unwritten-methods.tsv`, column "may be this method"): each is written.
#[test]
fn a_method_qml_may_reach_is_what_qt_has() {
    if !qt_log::qt_available() {
        return;
    }
    let file = fixture("may.qml");
    let (qt, ours) = (qt_log::qt_log(&file), qt_log::our_log(&file));
    compare("Qt 6.12", &qt, &ours);
    assert_eq!(entries(&qt), entries(&ours));
}

/// The `console.*` lines of a shell in Quickshell (ri-testenv), up to its `done`.
fn quickshell_log(shell: &std::path::Path) -> Vec<String> {
    use std::io::BufRead;
    let mut qs = std::process::Command::new("qs")
        .arg("-p")
        .arg(shell)
        .args(["--no-color", "--log-rules", "*.debug=false;qml.debug=true"])
        .env("QT_QPA_PLATFORM", "offscreen")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut out = Vec::new();
    for l in std::io::BufReader::new(qs.stdout.take().unwrap()).lines() {
        let l = l.unwrap();
        // "  DEBUG qml: text"; a later line of the same message has no prefix.
        let text = match l.split_once(" qml: ") {
            Some((level, text)) if level.trim() == "DEBUG" => text,
            Some(_) => continue,
            None if l.starts_with("  ") && !l.starts_with("    ") => continue,
            None => &l,
        };
        if text == "done" {
            break;
        }
        out.push(text.to_owned());
    }
    qs.kill().unwrap();
    qs.wait().unwrap();
    out
}

/// The same for the Quickshell types `qml/` may reach.
#[test]
fn a_quickshell_method_qml_may_reach_is_what_quickshell_has() {
    if !std::path::Path::new("/usr/bin/qs").exists() {
        testenv::unavailable(format_args!("`qs` (Quickshell) is not installed"));
        return;
    }
    let file = fixture("quickshell/shell.qml");
    let (qs, mut ours) = (quickshell_log(&file), qt_log::our_log(&file));
    assert_eq!(ours.pop().as_deref(), Some("done"));
    compare("Quickshell", &qs, &ours);
    assert_eq!(entries(&qs), entries(&ours));
}
