//! Files whose header claims a huge size are refused before any pixel buffer is allocated (a
//! failed allocation would abort the whole shell, which runs these decoders in process).

use imgtools::{busy, cv, limit, magick};

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

/// A ~100-byte PNG whose IHDR says `w`x`h` at `depth` bits, `color` type.
fn png_bomb(w: u32, h: u32, depth: u8, color: u8) -> Vec<u8> {
    let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([depth, color, 0, 0, 0]);
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

fn thumb(data: &[u8], name: &str) -> magick::Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "imgtools-decode-limit-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join(name);
    std::fs::write(&src, data).unwrap();
    let r = magick::resize_to_png(&src, 256, &dir.join(format!("{name}.png")));
    let _ = std::fs::remove_dir_all(&dir);
    r
}

fn refused(r: magick::Result<()>) {
    let e = r.expect_err("bomb decoded");
    assert!(e.to_string().contains("decode limit"), "{e}");
}

#[test]
fn check_size_counts_qt_32bit_pixels() {
    assert!(limit::check_size(8192, 8192, 3).is_ok());
    assert!(limit::check_size(8192, 8192, 4).is_ok());
    assert!(limit::check_size(8193, 8192, 3).is_err());
    assert!(limit::check_size(8192, 8192, 8).is_err());
    assert!(limit::check_size(usize::MAX, 2, 4).is_err());
}

#[test]
fn png_bomb_is_refused_from_its_header() {
    // RGBA16 65535x65535: a 34 GB frame buffer, which used to abort the process.
    let bomb = png_bomb(65535, 65535, 16, 6);
    assert!(bomb.len() < 200);
    refused(thumb(&bomb, "bomb.png"));
    assert!(busy::imdecode_gray(&bomb).is_none());
    assert!(cv::imdecode_bgr(&bomb).is_none());
}

#[test]
fn png_16bit_frame_over_limit_is_refused() {
    // 8192x8192 passes the 32-bit pixel count, but its RGBA16 buffer is 512 MB.
    refused(thumb(&png_bomb(8192, 8192, 16, 6), "wide.png"));
}

#[test]
fn jpeg_bomb_is_refused_from_its_header() {
    let bomb = jpeg_bomb(65500, 65500);
    refused(thumb(&bomb, "bomb.jpg"));
    assert!(busy::imdecode_gray(&bomb).is_none());
    assert!(cv::imdecode_bgr(&bomb).is_none());
}

#[test]
fn other_format_bomb_is_refused_from_its_header() {
    // A BMP header for 60000x60000 32-bit pixels and no pixel data.
    let (w, h) = (60000i32, 60000i32);
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
    refused(thumb(&b, "bomb.bmp"));
    assert!(cv::imdecode_bgr(&b).is_none());
}

#[test]
fn small_images_still_decode() {
    let mut px = [0u8; 4 * 4 * 3];
    px[0] = 255;
    let img = turbojpeg::Image {
        pixels: &px[..],
        width: 4,
        pitch: 12,
        height: 4,
        format: turbojpeg::PixelFormat::RGB,
    };
    let j = turbojpeg::compress(img, 90, turbojpeg::Subsamp::None).unwrap();
    thumb(&j, "ok.jpg").unwrap();
    assert!(busy::imdecode_gray(&j).is_some());
    assert_eq!(cv::imdecode_bgr(&j).unwrap().w, 4);
}
