fn main() {
    for p in std::env::args().skip(1) {
        let raw = std::fs::read(format!("{p}.pyraw")).unwrap();
        let q = colorgen::quantize::celebi(raw.chunks(4), 128);
        let js = std::fs::read_to_string(format!("{p}.py.json")).unwrap();
        let i = js.find("\"q\": [").unwrap() + 6;
        let j = js[i..].find("]], \"score\"").unwrap() + i + 1;
        let py: Vec<(u32, u32)> = js[i..j]
            .split("], [")
            .map(|e| {
                let e = e.trim_matches(|c| c == '[' || c == ']');
                let mut it = e.split(", ");
                (
                    it.next().unwrap().parse().unwrap(),
                    it.next().unwrap().parse().unwrap(),
                )
            })
            .collect();
        println!("{p}: eq={} n={} py_n={}", q == py, q.len(), py.len());
    }
}
