//! Equivalence tests of `imgtools::busy` against the original's `scripts/images/least_busy_region.py`.
//! Goldens come from `tests/busy_golden.py` (OpenCV 4.13.0 / numpy 2.3.5).

use imgtools::busy::{cli, imdecode_gray};
use imgtools::cv::imdecode_bgr;
use md5::{Digest, Md5};
use rayon::prelude::*;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn cli_matches_script_goldens() {
    let gold = std::fs::read_to_string(dir().join("busy_golden.tsv")).unwrap();
    let fix = dir().join("fixtures/busy");
    let lines: Vec<&str> = gold.lines().collect();
    let n = lines.len();
    let bad: Vec<String> = lines
        .par_iter()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let mut argv = vec![fix.join(f[0]).to_string_lossy().into_owned()];
            argv.extend(
                f[3..]
                    .iter()
                    .filter(|a| !a.is_empty())
                    .map(|a| a.to_string()),
            );
            let want = (unescape(f[2]), f[1].parse::<i32>().unwrap());
            let got = cli(&argv);
            (got != want).then(|| {
                format!(
                    "{} {:?}\n  want {:?}\n  got  {:?}",
                    f[0],
                    &f[3..],
                    want,
                    got
                )
            })
        })
        .collect();
    assert!(n > 300);
    assert!(
        bad.is_empty(),
        "{} of {n} differ:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

fn md5(w: usize, h: usize, c: usize, px: &[u8]) -> String {
    format!("{w}x{h}x{c}:{:x}", Md5::digest(px))
}

/// Intermediate stages (decode, resize + crop), to localise a mismatch.
#[test]
fn decode_and_resize_stages_match_opencv() {
    let rows = std::fs::read_to_string(dir().join("busy_stages.tsv")).unwrap();
    let mut bad = Vec::new();
    for line in rows.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let b = std::fs::read(dir().join("fixtures/busy").join(f[0])).unwrap();
        let (sw, sh) = f[1].split_once('x').unwrap();
        let (sw, sh): (i64, i64) = (sw.parse().unwrap(), sh.parse().unwrap());
        let fit = f[2] == "fit";
        let g = imdecode_gray(&b).unwrap();
        let c = imdecode_bgr(&b).unwrap();
        let (gw, gh, gpx) = imgtools::busy::scaled_for_test(g.w, g.h, 1, &g.px, sw, sh, fit);
        let (cw, ch, cpx) = imgtools::busy::scaled_for_test(c.w, c.h, 3, &c.px, sw, sh, fit);
        let got = [
            md5(g.w, g.h, 1, &g.px),
            md5(gw, gh, 1, &gpx),
            md5(c.w, c.h, 3, &c.px),
            md5(cw, ch, 3, &cpx),
        ];
        for (i, name) in ["gray", "gray scaled", "color", "color scaled"]
            .iter()
            .enumerate()
        {
            if got[i] != f[3 + i] {
                bad.push(format!(
                    "{} {} {}: {name} want {} got {}",
                    f[0],
                    f[1],
                    f[2],
                    f[3 + i],
                    got[i]
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
