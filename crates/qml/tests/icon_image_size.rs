//! The size an `IconImage` asks its image at when its own size is not whole, against
//! Quickshell 0.3.2 (ri-testenv). Quickshell's `IconImage.qml` binds `sourceSize.width` and
//! `.height`, which are ints, to the real `actualSize`. Its module is compiled ahead of time,
//! and that code cuts the fraction: the 19.5 wide icon of the bar's left button
//! (`qml/modules/rusty/bar/LeftSidebarButton.qml`) is asked at 19x19 and stretched to 19.5.
//! The same binding in a document Qt interprets rounds, to 20x20.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

/// The `console.*` lines of a shell in Quickshell, up to its `done`.
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
        let Some((level, text)) = l.split_once(" qml: ") else {
            continue;
        };
        if level.trim() != "DEBUG" {
            continue;
        }
        if text == "done" {
            break;
        }
        out.push(text.to_owned());
    }
    qs.kill().unwrap();
    qs.wait().unwrap();
    out
}

#[test]
fn an_icon_image_asks_its_image_at_its_size_without_the_fraction() {
    if !std::path::Path::new("/usr/bin/qs").exists() {
        testenv::unavailable(format_args!("`qs` (Quickshell) is not installed"));
        return;
    }
    let file =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/icon_image_size/shell.qml");
    let (qs, mut ours) = (quickshell_log(&file), qt_log::our_log(&file));
    assert_eq!(ours.pop().as_deref(), Some("done"));
    assert_eq!(
        qs,
        [
            "IconImage 19.5: 19x19",
            "IconImage 19.7 by 30: 19x19",
            "IconImage 30 by 20.2: 20x20",
            "IconImage implicitSize 21.99: 21x21",
            "IconImage implicitSize 24: 24x24",
            "Image 19.5: 20x20",
        ],
        "Quickshell"
    );
    assert_eq!(ours, qs);
}
