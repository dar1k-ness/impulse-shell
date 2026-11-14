use colorgen::scss::{TermOpts, generate, parse_hex};
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let term: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(std::env::args().nth(2).unwrap()).unwrap())
            .unwrap();
    let (mut ok, mut bad) = (0, 0);
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.starts_with("c_"))
        .collect();
    names.sort();
    for n in names {
        let parts: Vec<&str> = n.trim_end_matches(".scss").splitn(4, '_').collect();
        let (hexs, rest) = (parts[1], &n[3 + 6..n.len() - 5]);
        let (scheme, mode) = rest.rsplit_once('_').unwrap();
        let want = std::fs::read_to_string(format!("{dir}/{n}")).unwrap();
        let opts = TermOpts {
            blend_bg_fg: true,
            ..Default::default()
        };
        let got = generate(
            parse_hex(&format!("#{hexs}")).unwrap(),
            scheme,
            mode == "dark",
            Some(&term),
            &opts,
        )
        .unwrap_or_default();
        if got == want {
            ok += 1
        } else {
            bad += 1;
            if bad <= 6 {
                let d: Vec<String> = want
                    .lines()
                    .zip(got.lines())
                    .filter(|(a, b)| a != b)
                    .take(4)
                    .map(|(a, b)| format!("{a} | {b}"))
                    .collect();
                println!("DIFF {n}: {:?}", d);
            }
        }
    }
    println!("ok={ok} bad={bad}");
}
