//! The tree's terminal sequences template as `apply_anyterm` renders it (end-4/dots-hyprland
//! #3626): every OSC must end with ST (`ESC \`) or BEL, or terminals stay inside the last
//! sequence and freeze.

use colorgen::apply::{TERM_ALPHA, fill, scss_pairs};
use std::path::Path;

const II: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml");
const SCSS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/palette/scss/c_000000_auto_dark.scss"
);

#[test]
fn rendered_sequences_terminate_every_osc() {
    let tpl = std::fs::read_to_string(Path::new(II).join("scripts/colors/terminal/sequences.txt"))
        .unwrap();
    let pairs = scss_pairs(&std::fs::read_to_string(SCSS).unwrap());
    let out = fill(&tpl, &pairs).replace("$alpha", TERM_ALPHA);
    assert!(!out.contains("#$"), "unfilled colors: {out:?}");
    let b = out.as_bytes();
    let mut oscs = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1b && b.get(i + 1) == Some(&b']') {
            oscs += 1;
            let body = &b[i + 2..];
            let end = body
                .iter()
                .position(|&c| c == 0x1b || c == 0x07)
                .unwrap_or_else(|| panic!("OSC #{oscs} never ends"));
            let st = body[end] == 0x07 || body.get(end + 1) == Some(&b'\\');
            assert!(
                st,
                "OSC #{oscs} {:?} not terminated by ST",
                String::from_utf8_lossy(&body[..end])
            );
            i += 2 + end + if body[end] == 0x07 { 1 } else { 2 };
        } else {
            i += 1;
        }
    }
    assert!(oscs > 16, "{oscs} OSCs");
    assert!(out.ends_with("\x1b\\"), "{:?}", &out[out.len() - 20..]);
}
