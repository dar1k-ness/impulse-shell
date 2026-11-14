//! A wallpaper whose header claims a huge size is refused with an ordinary error before any
//! pixel buffer is allocated: the colour generation runs inside the shell process, which a
//! failed (or a 12 GB) allocation would take down.

use colorgen::{colorfulness, mg, seed};

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

/// A ~100-byte RGBA16 PNG whose IHDR says `w`x`h`.
fn png_bomb(w: u32, h: u32) -> Vec<u8> {
    let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([16, 6, 0, 0, 0]);
    chunk(&mut p, b"IHDR", &ihdr);
    chunk(
        &mut p,
        b"IDAT",
        &[0x78, 0x9c, 0x63, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01],
    );
    chunk(&mut p, b"IEND", &[]);
    p
}

/// A real 16x16 JPEG with its SOF0 size patched to `w`x`h`.
fn jpeg_bomb(w: u16, h: u16) -> Vec<u8> {
    let px = vec![128u8; 16 * 16 * 3];
    let img = turbojpeg::Image {
        pixels: &px[..],
        width: 16,
        pitch: 16 * 3,
        height: 16,
        format: turbojpeg::PixelFormat::RGB,
    };
    let mut j = turbojpeg::compress(img, 90, turbojpeg::Subsamp::None)
        .unwrap()
        .to_vec();
    let sof = j.windows(2).position(|m| m == [0xFF, 0xC0]).unwrap();
    j[sof + 5..sof + 7].copy_from_slice(&h.to_be_bytes());
    j[sof + 7..sof + 9].copy_from_slice(&w.to_be_bytes());
    j
}

/// A GIF of two 1x1 frames on a `w`x`h` canvas.
fn gif(w: u16, h: u16) -> Vec<u8> {
    let mut g = b"GIF89a".to_vec();
    g.extend(w.to_le_bytes());
    g.extend(h.to_le_bytes());
    g.extend([0x80, 0, 0]); // a 2-colour global table
    g.extend([200, 30, 30, 30, 30, 200]);
    for _ in 0..2 {
        g.extend([0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        g.extend([2, 2, 0x44, 0x01, 0]);
    }
    g.push(0x3B);
    g
}

/// A BMP header for `w`x`h` 32-bit pixels and no pixel data.
fn bmp_bomb(w: i32, h: i32) -> Vec<u8> {
    let mut b = b"BM".to_vec();
    b.extend(0u32.to_le_bytes()); // file size
    b.extend(0u32.to_le_bytes()); // reserved
    b.extend(54u32.to_le_bytes());
    b.extend(40u32.to_le_bytes());
    b.extend(w.to_le_bytes());
    b.extend(h.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(32u16.to_le_bytes());
    b.extend([0u8; 24]);
    b
}

fn heic() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../qsg/tests/fixtures/heif/quad.heic"
    );
    std::fs::read(path).unwrap()
}

/// The 32x16 HEIC fixture with the size in its `ispe` boxes patched to `w`x`h`.
fn heic_bomb(w: u32, h: u32) -> Vec<u8> {
    let mut d = heic();
    let at: Vec<usize> = (0..d.len() - 4)
        .filter(|&i| &d[i..i + 4] == b"ispe")
        .collect();
    assert!(!at.is_empty());
    for i in at {
        d[i + 8..i + 12].copy_from_slice(&w.to_be_bytes());
        d[i + 12..i + 16].copy_from_slice(&h.to_be_bytes());
    }
    d
}

/// Each of the three decoders of a wallpaper refuses `bomb` for its size.
fn refused(name: &str, bomb: &[u8]) {
    let over = |e: String| assert!(e.contains("decode limit"), "{name}: {e}");
    over(
        colorfulness::imread(bomb)
            .err()
            .unwrap_or_else(|| panic!("{name}: imread decoded a bomb"))
            .0,
    );
    over(
        seed::seed_from_bytes(bomb)
            .err()
            .unwrap_or_else(|| panic!("{name}: seed decoded a bomb"))
            .0,
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, bomb).unwrap();
    over(
        mg::source_from_image(&path)
            .err()
            .unwrap_or_else(|| panic!("{name}: matugen source decoded a bomb"))
            .0,
    );
    // What the script printed when its decoder raised.
    assert_eq!(colorfulness::scheme_for_image(&path), "scheme-tonal-spot");
}

#[test]
fn jpeg_bomb_is_refused_from_its_header() {
    // 65500x65500: a 12.5 GB RGB buffer, which took the shell down.
    let bomb = jpeg_bomb(65500, 65500);
    assert!(bomb.len() < 2000);
    refused("bomb.jpg", &bomb);
}

#[test]
fn png_bomb_is_refused_from_its_header() {
    refused("bomb.png", &png_bomb(65535, 65535));
}

#[test]
fn gif_bomb_is_refused_from_its_header() {
    // Every frame is composed on the canvas: 65535x65535 RGBA is 16 GB.
    refused("bomb.gif", &gif(65535, 65535));
}

#[test]
fn bmp_bomb_is_refused_from_its_header() {
    refused("bomb.bmp", &bmp_bomb(60000, 60000));
}

#[test]
fn heic_bomb_is_refused_from_its_header() {
    // libheif's own limit is 32768x32768 pixels, 3 GB of RGB.
    let bomb = heic_bomb(30000, 30000);
    let e = seed::seed_from_bytes(&bomb).expect_err("decoded a bomb");
    assert!(e.0.contains("decode limit"), "{}", e.0);
}

#[test]
fn images_within_the_limit_still_decode() {
    for (name, data) in [("ok.jpg", jpeg_bomb(16, 16)), ("ok.gif", gif(40, 30))] {
        let img = colorfulness::imread(&data).unwrap_or_else(|e| panic!("{name}: {}", e.0));
        assert_eq!(
            (img.width, img.height),
            if name == "ok.jpg" { (16, 16) } else { (40, 30) }
        );
        seed::seed_from_bytes(&data).unwrap_or_else(|e| panic!("{name}: {}", e.0));
    }
    let heic = seed::decode(&heic()).unwrap_or_else(|e| panic!("heic: {}", e.0));
    assert_eq!((heic.width, heic.height), (32, 16));
}
