//! The size of an image file, for the original's `ImageSize`, which reads it as one `W H` pair. It is
//! the size the image is shown at: the canvas of an animation, else the first image in the
//! file (ImageMagick's `identify`, which the original ran before, printed one pair per GIF/WebP frame,
//! TIFF page, ICO entry or PSD layer, and the original misread `100 8040 30` as 100×8040). Every format
//! ImageMagick reads by content is read with the crate that decodes it (AVIF/HEIC with the
//! libheif ImageMagick itself uses); anything else has no size.

use crate::jobs::Job;
use std::fs::File;
use std::io::{BufReader, Read};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Still,
    Pnm,
    Gif,
    WebP,
    Tiff,
    Ico,
    Heif,
    Svg,
}

fn kind(file: &str) -> Option<Kind> {
    use imagesize::ImageType as T;
    let mut head = [0u8; 256];
    let n = File::open(file).and_then(|mut f| f.read(&mut head)).ok()?;
    let head = &head[..n];
    match imagesize::reader_type(BufReader::new(File::open(file).ok()?)) {
        Ok(
            T::Png | T::Jpeg | T::Bmp | T::Qoi | T::Farbfeld | T::Hdr | T::Dds(_) | T::Jxl | T::Psd,
        ) => Some(Kind::Still),
        // ImageMagick has no content test for Targa: only a `.tga` (or alias) name selects it.
        Ok(T::Tga) => {
            let ext = std::path::Path::new(file)
                .extension()?
                .to_str()?
                .to_ascii_lowercase();
            ["tga", "icb", "vda", "vst"]
                .contains(&ext.as_str())
                .then_some(Kind::Still)
        }
        Ok(T::Pnm) => Some(Kind::Pnm),
        Ok(T::Gif) => Some(Kind::Gif),
        Ok(T::Webp) => Some(Kind::WebP),
        Ok(T::Tiff) => Some(Kind::Tiff),
        Ok(T::Ico) => Some(Kind::Ico),
        Ok(T::Heif(_)) => Some(Kind::Heif),
        // imagesize does not know PAM.
        _ if head.starts_with(b"P7\n") => Some(Kind::Pnm),
        _ => is_svg(head).then_some(Kind::Svg),
    }
}

/// ImageMagick's `IsSVG`: `<svg` or `<?xml` at the start.
fn is_svg(head: &[u8]) -> bool {
    let h = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
    let h = &h[h
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(h.len())..];
    h.starts_with(b"<svg") || h.starts_with(b"<?xml")
}

/// `ImageSize`: `W H` on stdout and code 0, or code 1 and nothing when `file` has no size.
pub fn job(file: &str) -> Option<Job> {
    let file = file.to_owned();
    Some(Box::new(move |io, _| match size_of(&file) {
        Some((w, h)) => {
            io.out(format!("{w} {h}"));
            0
        }
        None => 1,
    }))
}

/// Width and height of the image in `file`; `None` when it is not an image this reads.
pub fn size_of(file: &str) -> Option<(u64, u64)> {
    kind(file).and_then(|k| size(file, k))
}

fn size(file: &str, k: Kind) -> Option<(u64, u64)> {
    let open = || File::open(file).ok().map(BufReader::new);
    let s = match k {
        Kind::Still => {
            let s = imagesize::size(file).ok()?;
            (s.width as u64, s.height as u64)
        }
        Kind::Pnm => {
            use image::ImageDecoder;
            let (w, h) = image::codecs::pnm::PnmDecoder::new(open()?)
                .ok()?
                .dimensions();
            (w as u64, h as u64)
        }
        Kind::Gif => {
            let d = gif::DecodeOptions::new().read_info(open()?).ok()?;
            (d.width() as u64, d.height() as u64)
        }
        Kind::WebP => webp_canvas(&std::fs::read(file).ok()?)?,
        Kind::Tiff => {
            let (w, h) = tiff::decoder::Decoder::new(open()?)
                .ok()?
                .dimensions()
                .ok()?;
            (w as u64, h as u64)
        }
        Kind::Ico => {
            let d = ico::IconDir::read(open()?).ok()?;
            let e = d.entries().first()?;
            (e.width() as u64, e.height() as u64)
        }
        Kind::Heif => heif_size(file)?,
        Kind::Svg => svg_size(&std::fs::read_to_string(file).ok()?)?,
    };
    (s.0 > 0 && s.1 > 0).then_some(s)
}

/// The canvas size from libwebp's demuxer (ImageMagick's WebP coder reads the same).
fn webp_canvas(b: &[u8]) -> Option<(u64, u64)> {
    use libwebp_sys as w;
    let data = w::WebPData {
        bytes: b.as_ptr(),
        size: b.len(),
    };
    // SAFETY: `data` points at `b`, which outlives the demuxer (deleted below); a null state
    // is allowed, and the ABI version is the one the bindings were generated for.
    let dmux = unsafe {
        w::WebPDemuxInternal(
            &data,
            0,
            std::ptr::null_mut(),
            w::WEBP_DEMUX_ABI_VERSION as _,
        )
    };
    if dmux.is_null() {
        return None;
    }
    // SAFETY: `dmux` is the live demuxer created above.
    let width = unsafe { w::WebPDemuxGetI(dmux, w::WebPFormatFeature::WEBP_FF_CANVAS_WIDTH) };
    // SAFETY: as above.
    let height = unsafe { w::WebPDemuxGetI(dmux, w::WebPFormatFeature::WEBP_FF_CANVAS_HEIGHT) };
    // SAFETY: `dmux` came from WebPDemuxInternal and is not used after this.
    unsafe { w::WebPDemuxDelete(dmux) };
    Some((width as u64, height as u64))
}

/// The primary image's size after its transformations (`irot`, `imir`, `clap`), from libheif:
/// ImageMagick reads AVIF and HEIC only through libheif, so without it both fail.
fn heif_size(file: &str) -> Option<(u64, u64)> {
    let (w, h) = heif::file_size(file).ok()?;
    Some((w as u64, h as u64))
}

/// The size librsvg gives ImageMagick's SVG coder at its default 96 DPI, rounded half up: the
/// root's absolute `width`/`height`, a missing one from the `viewBox` aspect, the `viewBox`
/// alone, or with neither the drawing's extent.
fn svg_size(text: &str) -> Option<(u64, u64)> {
    use svgtypes::LengthUnit as U;
    let doc = roxmltree::Document::parse(text).ok()?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return None;
    }
    let px = |name: &str| -> Option<f64> {
        let l: svgtypes::Length = root.attribute(name)?.parse().ok()?;
        let k = match l.unit {
            U::None | U::Px => 1.0,
            U::In => 96.0,
            U::Cm => 96.0 / 2.54,
            U::Mm => 96.0 / 25.4,
            U::Pt => 96.0 / 72.0,
            U::Pc => 16.0,
            U::Em => 12.0,
            U::Ex => 6.0,
            U::Percent => return None,
        };
        Some(l.number * k)
    };
    let vb = root
        .attribute("viewBox")
        .and_then(|v| v.parse::<svgtypes::ViewBox>().ok())
        .filter(|v| v.w > 0.0 && v.h > 0.0);
    let (w, h) = match (px("width"), px("height"), vb) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(v)) => (w, w * v.h / v.w),
        (None, Some(h), Some(v)) => (h * v.w / v.h, h),
        (_, _, Some(v)) => (v.w, v.h),
        (_, _, None) => {
            let t = usvg::Tree::from_str(text, &usvg::Options::default()).ok()?;
            let b = t.root().abs_bounding_box();
            (b.right() as f64, b.bottom() as f64)
        }
    };
    let round = |v: f64| (v + 0.5).floor();
    (w > 0.0 && h > 0.0).then(|| (round(w) as u64, round(h) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn have(t: &str) -> bool {
        Command::new(t)
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// ImageMagick's size of the first image (`file[0]`), or of an animation's canvas, for
    /// stills, animated GIF/WebP whose frames differ in size, multi-image TIFF/ICO/PSD, every
    /// other format it reads by content, SVG sizing, misnamed files and the open error.
    #[test]
    fn sizes_match_imagemagick() {
        if !have("magick") {
            return crate::equiv_missing("ImageMagick 7 (magick, identify)");
        }
        let d = std::env::temp_dir().join(format!("ri-identify-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mk = |args: &[&str]| {
            assert!(
                Command::new("magick")
                    .args(args)
                    .current_dir(&d)
                    .status()
                    .unwrap()
                    .success(),
                "{args:?}"
            )
        };
        mk(&["-size", "123x45", "xc:red", "a.png"]);
        mk(&["-size", "123x45", "xc:red", "-depth", "16", "a16.png"]);
        mk(&["-size", "77x1031", "xc:red", "a.jpg"]);
        mk(&["-size", "77x31", "xc:red", "-orient", "RightTop", "o.jpg"]);
        mk(&["-size", "31x7", "xc:red", "a.bmp"]);
        mk(&["-size", "64x48", "xc:red", "a.gif"]);
        mk(&["-size", "64x48", "xc:red", "-quality", "50", "a.webp"]);
        mk(&[
            "-size",
            "65x47",
            "xc:red",
            "-define",
            "webp:lossless=true",
            "l.webp",
        ]);
        mk(&[
            "-dispose",
            "none",
            "-delay",
            "10",
            "-size",
            "100x80",
            "xc:red",
            "-page",
            "100x80+10+5",
            "-size",
            "40x30",
            "xc:blue",
            "-page",
            "100x80+1+2",
            "-size",
            "13x9",
            "xc:green",
            "-loop",
            "0",
            "g.gif",
        ]);
        mk(&["g.gif", "w.webp"]);
        mk(&[
            "-size", "30x20", "xc:red", "-size", "10x40", "xc:blue", "m.tiff",
        ]);
        mk(&[
            "-size", "16x16", "xc:red", "-size", "32x32", "xc:blue", "-size", "256x256",
            "xc:green", "m.ico",
        ]);
        mk(&[
            "-size", "30x20", "xc:red", "-size", "7x9", "xc:blue", "p.psd",
        ]);
        mk(&["-size", "30x20", "xc:red", "-rotate", "90", "r.avif"]);
        mk(&["a.jpg", "j.png"]);
        for ext in [
            "qoi", "tga", "ppm", "pgm", "pbm", "pam", "ff", "hdr", "dds", "psd", "jxl", "avif",
            "heic", "tiff", "ico",
        ] {
            mk(&["a.png", &format!("a.{ext}")]);
        }
        std::fs::copy(d.join("a.tga"), d.join("tga.png")).unwrap();
        let svg = |name: &str, attrs: &str| {
            let ns = if name == "nons.svg" {
                ""
            } else {
                " xmlns=\"http://www.w3.org/2000/svg\""
            };
            std::fs::write(d.join(name), format!("<?xml version=\"1.0\"?>\n<svg{ns} {attrs}><rect width=\"5\" height=\"5\"/></svg>")).unwrap();
        };
        svg("units.svg", "width=\"10mm\" height=\"2in\"");
        svg("pt.svg", "width=\"10.4pt\" height=\"3pc\"");
        svg("em.svg", "width=\"2em\" height=\"3ex\"");
        svg("px.svg", "width=\"10.4\" height=\"10.5px\"");
        svg("vb.svg", "viewBox=\"0 0 33.5 21\"");
        svg("pct.svg", "width=\"50%\" viewBox=\"0 0 40 20\"");
        svg("wonly.svg", "width=\"20\" viewBox=\"0 0 40 20\"");
        svg("nodims.svg", "");
        svg("nons.svg", "width=\"8\" height=\"9\"");
        std::fs::write(d.join("x.xml"), "<?xml version=\"1.0\"?><a/>").unwrap();
        std::fs::write(d.join("t.txt"), "hello\n").unwrap();
        std::fs::write(d.join("fake.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        let mut files: Vec<String> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        files.extend(["missing.png".into(), String::new()]);
        let mut bad = vec![];
        for f in &files {
            let p = if f.is_empty() {
                String::new()
            } else {
                d.join(f).to_string_lossy().into_owned()
            };
            // The size the original means: the first image, or an animation's canvas (page).
            let animated = f.ends_with(".gif") || f.ends_with(".webp");
            let fmt = if animated { "%W %H" } else { "%w %h" };
            let real = if p.is_empty() {
                (1, String::new())
            } else {
                let o = Command::new("magick")
                    .args(["identify", "-format", fmt, &format!("{p}[0]")])
                    .output()
                    .unwrap();
                (
                    o.status.code().unwrap(),
                    String::from_utf8(o.stdout).unwrap(),
                )
            };
            let ours = match size_of(&p) {
                Some((w, h)) => (0, format!("{w} {h}")),
                None => (1, String::new()),
            };
            if ours != real {
                bad.push(format!("{f}: ours {ours:?}, ImageMagick {real:?}"));
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
