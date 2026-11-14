//! What `matugen --source-color-index 0 ...` (4.2.0) computes and writes for the original templates:
//! the scheme comes straight from `material-colors` 0.4.2 (the library matugen is built on),
//! the templates use the `{{colors.<name>.<light|dark|default>.<format>}}` / `{{image}}` subset.

use material_colors::color::Argb;
use material_colors::dynamic_color::{DynamicScheme, Variant};
use material_colors::hct::Cam16;
use material_colors::quantize::{Quantizer, QuantizerCelebi};
use material_colors::scheme::Scheme;
use material_colors::score::Score;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct MgError(pub String);

fn err(e: impl std::fmt::Display) -> MgError {
    MgError(e.to_string())
}

/// matugen's `--type` (unset behaves as `scheme-tonal-spot` in matugen 4.2.0).
pub fn variant(scheme: Option<&str>) -> Variant {
    match scheme.unwrap_or("scheme-tonal-spot") {
        "scheme-content" => Variant::Content,
        "scheme-expressive" => Variant::Expressive,
        "scheme-fidelity" => Variant::Fidelity,
        "scheme-fruit-salad" => Variant::FruitSalad,
        "scheme-monochrome" => Variant::Monochrome,
        "scheme-neutral" => Variant::Neutral,
        "scheme-rainbow" => Variant::Rainbow,
        "scheme-vibrant" => Variant::Vibrant,
        _ => Variant::TonalSpot,
    }
}

/// `get_scored_colors_from_image(...)[0]`.
pub fn source_from_image(path: &Path) -> Result<Argb, MgError> {
    let img = match heif_image(path)? {
        Some(img) => img,
        None => imgtools::limit::image_file(path).map_err(err)?,
    };
    // material-colors' `image::Image::resize` + `AsPixels` (its `image` feature would pull in
    // image's default formats, the AVIF encoder rav1e included). The filter treats channels
    // alike, so an opaque image is resized as RGB: same colours, no full-size RGBA copy.
    let pixels = if img.color().has_alpha() {
        small_pixels(&img.into_rgba8(), |[r, g, b, a]| Argb::new(a, r, g, b))
    } else {
        small_pixels(&img.into_rgb8(), |[r, g, b]| Argb::new(255, r, g, b))
    };
    quantized_source(&pixels)
}

/// An AVIF or HEIC file, which matugen 4.2.0 itself does not read (the original's switchwall.sh ignores
/// its failure and leaves its templates stale): the pixels `seed` gets, so these wallpapers
/// recolour the templates too. `None` for any other file.
fn heif_image(path: &Path) -> Result<Option<image::DynamicImage>, MgError> {
    use std::io::Read;
    let mut head = [0u8; 8];
    let mut file = std::fs::File::open(path).map_err(err)?;
    if file.read_exact(&mut head).is_err() || &head[4..] != b"ftyp" {
        return Ok(None);
    }
    let mut bytes = head.to_vec();
    file.read_to_end(&mut bytes).map_err(err)?;
    if !heif::is_heif(&bytes) {
        return Ok(None);
    }
    let px = crate::seed::decode(&bytes).map_err(|e| MgError(e.0))?;
    let (w, h) = (px.width as u32, px.height as u32);
    let img = if px.channels == 4 {
        image::RgbaImage::from_raw(w, h, px.data).map(image::DynamicImage::ImageRgba8)
    } else {
        image::RgbImage::from_raw(w, h, px.data).map(image::DynamicImage::ImageRgb8)
    };
    img.map(Some).ok_or_else(|| err("bad AVIF/HEIC buffer"))
}

fn small_pixels<P, const N: usize>(
    img: &image::ImageBuffer<P, Vec<u8>>,
    argb: impl Fn([u8; N]) -> Argb,
) -> Vec<Argb>
where
    P: image::Pixel<Subpixel = u8> + 'static,
{
    image::imageops::resize(img, 112, 112, image::imageops::FilterType::Triangle)
        .pixels()
        .map(|p| argb(p.channels().try_into().unwrap()))
        .filter(|a| a.alpha == 255)
        .collect()
}

fn quantized_source(pixels: &[Argb]) -> Result<Argb, MgError> {
    let mut result = QuantizerCelebi::quantize(pixels, 128);
    result
        .color_to_count
        .retain(|&argb, _| Cam16::from(argb).chroma >= 5.0);
    Score::score(&result.color_to_count, None, None, None)
        .first()
        .copied()
        .ok_or_else(|| err("no source color"))
}

pub struct Colors {
    pub source: Argb,
    pub dark_default: bool,
    pub light: Vec<(String, Argb)>,
    pub dark: Vec<(String, Argb)>,
}

impl Colors {
    pub fn new(source: Argb, variant: Variant, dark_default: bool) -> Colors {
        let scheme = |dark| {
            Scheme::from(DynamicScheme::by_variant(source, &variant, dark, None))
                .into_iter()
                .collect()
        };
        Colors {
            source,
            dark_default,
            light: scheme(false),
            dark: scheme(true),
        }
    }

    fn lookup(&self, name: &str, which: &str) -> Option<Argb> {
        if name == "source_color" {
            return Some(self.source);
        }
        let find = |v: &[(String, Argb)]| v.iter().find(|(k, _)| k == name).map(|(_, a)| *a);
        match which {
            "light" => find(&self.light),
            "dark" => find(&self.dark),
            "default" => find(if self.dark_default {
                &self.dark
            } else {
                &self.light
            }),
            _ => None,
        }
    }
}

fn format(c: Argb, f: &str) -> Option<String> {
    Some(match f {
        "hex" => format!("#{:02x}{:02x}{:02x}", c.red, c.green, c.blue),
        "hex_stripped" => format!("{:02x}{:02x}{:02x}", c.red, c.green, c.blue),
        "red" => c.red.to_string(),
        "green" => c.green.to_string(),
        "blue" => c.blue.to_string(),
        _ => return None,
    })
}

/// `image` is the canonical wallpaper path (`None` for a color source, rendered as `Null`).
pub fn render(template: &str, colors: &Colors, image: Option<&str>) -> Result<String, MgError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").ok_or_else(|| err("unclosed {{"))?;
        let expr = after[..end].trim();
        if expr == "image" {
            out.push_str(image.unwrap_or("Null"));
        } else {
            let parts: Vec<&str> = expr.split('.').collect();
            let value = match parts.as_slice() {
                ["colors", name, which, fmt] => {
                    colors.lookup(name, which).and_then(|c| format(c, fmt))
                }
                _ => None,
            };
            out.push_str(
                &value.ok_or_else(|| err(format!("unsupported template expression {expr:?}")))?,
            );
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

pub struct TemplateSpec {
    pub name: String,
    pub input: PathBuf,
    pub output: PathBuf,
}

fn expand(p: &str, home: &Path) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(r) => home.join(r),
        None => PathBuf::from(p),
    }
}

/// `[templates.*]` of matugen's `config.toml`, in matugen's order (sorted by `index`, then name
/// as its `HashMap`-free toml table iterates).
pub fn templates(config_toml: &str, home: &Path) -> Result<Vec<TemplateSpec>, MgError> {
    let v: toml::Table = config_toml.parse().map_err(err)?;
    let mut out = Vec::new();
    let Some(t) = v.get("templates").and_then(|t| t.as_table()) else {
        return Ok(out);
    };
    let mut items: Vec<(i64, &String, &toml::Value)> = t
        .iter()
        .map(|(k, v)| {
            (
                v.get("index").and_then(|i| i.as_integer()).unwrap_or(0),
                k,
                v,
            )
        })
        .collect();
    items.sort_by_key(|(i, _, _)| *i);
    for (_, name, v) in items {
        if v.get("enabled").and_then(|e| e.as_bool()) == Some(false) {
            continue;
        }
        let (Some(i), Some(o)) = (
            v.get("input_path").and_then(|s| s.as_str()),
            v.get("output_path").and_then(|s| s.as_str()),
        ) else {
            continue;
        };
        out.push(TemplateSpec {
            name: name.clone(),
            input: expand(i, home),
            output: expand(o, home),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RGBA path every image took before: decode, widen to RGBA, resize, keep opaque.
    fn via_rgba(path: &Path) -> Argb {
        let rgba = image::open(path).unwrap().into_rgba8();
        let pixels = small_pixels(&rgba, |[r, g, b, a]| Argb::new(a, r, g, b));
        quantized_source(&pixels).unwrap()
    }

    fn picture(w: u32, h: u32) -> image::RgbImage {
        image::RgbImage::from_fn(w, h, |x, y| {
            let n = (x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503)) >> 7;
            image::Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, (n & 0xff) as u8])
        })
    }

    #[test]
    fn opaque_images_pick_the_same_source_as_rgba() {
        let dir = std::env::temp_dir().join(format!("mg-rgb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (i, (w, h)) in [(640, 360), (113, 97), (1920, 1080)]
            .into_iter()
            .enumerate()
        {
            for ext in ["png", "jpg"] {
                let p = dir.join(format!("{i}.{ext}"));
                picture(w, h).save(&p).unwrap();
                assert_eq!(
                    source_from_image(&p).unwrap(),
                    via_rgba(&p),
                    "{}",
                    p.display()
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transparent_pixels_still_drop_out() {
        let dir = std::env::temp_dir().join(format!("mg-rgba-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.png");
        let img = image::RgbaImage::from_fn(400, 300, |x, y| {
            if x < 200 {
                image::Rgba([230, 20, 20, 0])
            } else {
                image::Rgba([20, (y % 200) as u8, 200, 255])
            }
        });
        img.save(&p).unwrap();
        assert_eq!(source_from_image(&p).unwrap(), via_rgba(&p));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
