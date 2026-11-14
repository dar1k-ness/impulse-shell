fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let g: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(format!("{dir}/cf.json")).unwrap()).unwrap();
    for p in std::env::args().skip(2) {
        let b = std::path::Path::new(&p)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let img = colorgen::colorfulness::load_and_resize(&std::fs::read(&p).unwrap()).unwrap();
        let want = std::fs::read(format!("{dir}/{b}.cvraw")).unwrap();
        let d = want.iter().zip(&img.data).filter(|(a, b)| a != b).count();
        let v = colorgen::colorfulness::colorfulness(&img);
        let wv: f64 = g[&b][0].as_str().unwrap().parse().unwrap();
        println!(
            "{b}: {}x{} px_diffs={d} len_eq={} value_eq={} {v:?} vs {wv:?}",
            img.width,
            img.height,
            want.len() == img.data.len(),
            v == wv
        );
    }
}
