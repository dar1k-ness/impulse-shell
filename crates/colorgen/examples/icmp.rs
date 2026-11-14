use colorgen::scss::{TermOpts, generate};
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let term: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(std::env::args().nth(2).unwrap()).unwrap())
            .unwrap();
    for p in std::env::args().skip(3) {
        let b = std::path::Path::new(&p)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let want_seed = std::fs::read_to_string(format!("{dir}/i_{b}.color")).unwrap();
        let seed = colorgen::seed::seed_from_path(p.as_ref()).unwrap();
        let got = generate(
            seed,
            "scheme-tonal-spot",
            true,
            Some(&term),
            &TermOpts {
                blend_bg_fg: true,
                ..Default::default()
            },
        )
        .unwrap();
        let want = std::fs::read_to_string(format!("{dir}/i_{b}.scss")).unwrap();
        println!(
            "{b}: seed {} want {} scss_eq={}",
            seed.to_hex_with_pound().to_uppercase(),
            want_seed,
            got == want
        );
    }
}
