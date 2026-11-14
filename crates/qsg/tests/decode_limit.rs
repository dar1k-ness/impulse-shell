//! Images whose header claims more than QImageReader's 256 MB allocation limit are refused
//! from the header, before the decoder allocates their pixels.
use std::io::ErrorKind;

fn tmp(name: &str, data: &[u8]) -> String {
    let p = std::env::temp_dir().join(format!("qsg-limit-{}-{name}", std::process::id()));
    std::fs::write(&p, data).unwrap();
    p.to_string_lossy().into_owned()
}

fn refused(name: &str, data: &[u8]) {
    let f = tmp(name, data);
    for r in [
        qsg::image::load_ex(&f).map(|_| ()),
        qsg::image::load_scaled(&f, 0.0).map(|_| ()),
    ] {
        let e = r.expect_err(name);
        assert_eq!(e.kind(), ErrorKind::OutOfMemory, "{name}: {e}");
    }
    let _ = std::fs::remove_file(f);
}

/// A real 8x8 JPEG whose SOF0 now claims 9000x9000 (324 MB as 32-bit pixels).
#[test]
fn oversized_jpeg_header_is_refused() {
    let img = turbojpeg::Image {
        pixels: &[128u8; 8 * 8 * 3][..],
        width: 8,
        pitch: 8 * 3,
        height: 8,
        format: turbojpeg::PixelFormat::RGB,
    };
    let mut j = turbojpeg::compress(img, 90, turbojpeg::Subsamp::None)
        .unwrap()
        .to_vec();
    let sof = j.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
    j[sof + 5..sof + 9].copy_from_slice(&[0x23, 0x28, 0x23, 0x28]);
    assert_eq!(
        turbojpeg::read_header(&j)
            .map(|h| (h.width, h.height))
            .unwrap(),
        (9000, 9000)
    );
    refused("big.jpg", &j);
}

/// A BMP header claiming 9000x9000 (under the image crate's own 512 MB default).
#[test]
fn oversized_bmp_header_is_refused() {
    let (w, h) = (9000u32, 9000u32);
    let mut b = b"BM".to_vec();
    b.extend((54 + w * h * 4).to_le_bytes());
    b.extend([0; 4]);
    b.extend(54u32.to_le_bytes());
    b.extend(40u32.to_le_bytes());
    b.extend(w.to_le_bytes());
    b.extend(h.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(32u16.to_le_bytes());
    b.extend([0; 24]);
    b.extend([0x7f; 64]);
    refused("big.bmp", &b);
}

#[test]
fn limit_is_qimagereader_default() {
    assert!(qsg::image::check_size(8192, 8192).is_ok());
    assert!(qsg::image::check_size(8193, 8192).is_err());
    assert!(qsg::image::check_size(usize::MAX, 2).is_err());
}
