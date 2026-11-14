fn main() {
    for p in std::env::args().skip(1) {
        let data = std::fs::read(&p).unwrap();
        let img: turbojpeg::Image<Vec<u8>> =
            turbojpeg::decompress(&data, turbojpeg::PixelFormat::RGB).unwrap();
        let py = std::fs::read(format!("{p}.full.pyraw")).unwrap();
        let d = py.iter().zip(&img.pixels).filter(|(a, b)| a != b).count();
        println!(
            "{p}: {}x{} pitch={} len_eq={} diffs={d}",
            img.width,
            img.height,
            img.pitch,
            py.len() == img.pixels.len()
        );
    }
}
