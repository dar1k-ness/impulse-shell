// mgcmp <golden out dir> <sandbox home>: re-render every case and diff against matugen's files.
use colorgen::mg::*;
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (out, home) = (Path::new(&a[1]), Path::new(&a[2]));
    let cfg = std::fs::read_to_string(home.join(".config/matugen/config.toml")).unwrap();
    let specs = templates(&cfg, home).unwrap();
    let (mut ok, mut bad) = (0, 0);
    let mut cases: Vec<_> = std::fs::read_dir(out)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    cases.sort();
    for c in cases {
        let n = c.file_name().unwrap().to_str().unwrap().to_string();
        let (kind, rest) = n.split_at(2);
        let (rest, mode) = rest.rsplit_once('_').unwrap();
        let (src, ty) = if rest.contains("_scheme-") {
            let (s, t) = rest.split_once("_scheme-").unwrap();
            (s.to_string(), Some(format!("scheme-{t}")))
        } else {
            (rest.trim_end_matches("_none").to_string(), None)
        };
        let (seed, image) = if kind == "c_" {
            (colorgen::scss::parse_hex(&format!("#{src}")).unwrap(), None)
        } else {
            let p =
                std::fs::canonicalize(format!("{}/b1/fix/{src}", std::env::var("HOME").unwrap()))
                    .unwrap();
            (
                source_from_image(&p).unwrap(),
                Some(p.to_str().unwrap().to_string()),
            )
        };
        let colors = Colors::new(seed, variant(ty.as_deref()), mode == "dark");
        for s in &specs {
            let t = std::fs::read_to_string(&s.input).unwrap();
            let got = render(&t, &colors, image.as_deref()).unwrap();
            let want = std::fs::read_to_string(c.join(s.output.strip_prefix(home).unwrap()))
                .unwrap_or_default();
            if got == want {
                ok += 1
            } else {
                bad += 1;
                if std::env::var("V").is_ok() {
                    let d: Vec<String> = want
                        .lines()
                        .zip(got.lines())
                        .filter(|(x, y)| x != y)
                        .take(3)
                        .map(|(x, y)| format!("{x} | {y}"))
                        .collect();
                    println!("DIFF {n} {}: {d:?}", s.name);
                }
            }
        }
    }
    println!("ok={ok} bad={bad}");
}
