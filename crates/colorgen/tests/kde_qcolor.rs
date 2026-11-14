//! `kde::accent` colour maths against the real Qt + KGuiAddons + Plasma code, on the
//! cases in `tests/kde/qcolor.txt` (regenerate with `tools/reference/kmyc-golden/qcolor.sh`).

use colorgen::kde::accent::{self, QColor};

fn c(v: &str) -> QColor {
    QColor::read(Some(v))
}

fn s(c: QColor) -> String {
    if c.is_valid() {
        c.kconfig()
    } else {
        "invalid".into()
    }
}

#[test]
fn matches_qt() {
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/kde/qcolor.txt"))
            .unwrap();
    let mut bad = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        let input: Vec<&str> = parts[0].split_whitespace().collect();
        let want: Vec<&str> = parts[1].split_whitespace().collect();
        let nums: Vec<f64> = parts[2]
            .split_whitespace()
            .map(|n| n.parse().unwrap())
            .collect();
        let (base, acc, bg) = (c(input[0]), c(input[1]), c(input[2]));
        let factor: f64 = input[3].parse().unwrap();
        let abg = accent::accent_background(acc, bg);
        let got = [
            s(accent::tint_color(base, acc, factor)),
            s(accent::lighten(acc)),
            s(accent::darken(acc)),
            s(accent::readable_link(acc, bg)),
            s(abg),
            s(accent::accent_foreground(abg, true)),
            s(accent::accent_foreground(base, false)),
        ];
        let names = [
            "tint",
            "lighten",
            "darken",
            "link",
            "accentbg",
            "fgactive",
            "fginactive",
        ];
        for ((n, g), w) in names.iter().zip(&got).zip(&want) {
            if g != w {
                bad.push(format!("{n}({}): got {g}, want {w}", parts[0].trim()));
            }
        }
        let (luma, contrast) = (accent::luma(acc), accent::contrast_ratio(bg, acc));
        if luma != nums[0] || contrast != nums[1] {
            bad.push(format!(
                "luma/contrast({}): got {luma} {contrast}, want {} {}",
                parts[0].trim(),
                nums[0],
                nums[1]
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{} mismatches, first:\n{}",
        bad.len(),
        bad[..bad.len().min(30)].join("\n")
    );
}
