fn main() {
    for p in std::env::args().skip(1) {
        let img = image::open(&p).unwrap();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let ch = if img.color().has_alpha() { 4 } else { 3 };
        let px = if ch == 4 {
            img.to_rgba8().into_raw()
        } else {
            img.to_rgb8().into_raw()
        };
        let (nw, nh) = colorgen::pillow::optimal_size(w, h, 128);
        let out = if nw < w || nh < h {
            colorgen::pillow::resize_bicubic(&px, ch, w, h, nw, nh)
        } else {
            px
        };
        let rgba: Vec<u8> = out
            .chunks(ch)
            .flat_map(|c| [c[0], c[1], c[2], if ch == 4 { c[3] } else { 255 }])
            .collect();
        let py = std::fs::read(format!("{p}.pyraw")).unwrap();
        let d = py.iter().zip(&rgba).filter(|(a, b)| a != b).count();
        println!("{p}: ch={ch} len_eq={} diffs={d}", py.len() == rgba.len());
    }
}
