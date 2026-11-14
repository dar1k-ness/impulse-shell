//! `generate_colors_material.py` output (`material_colors.scss`).

use crate::mdc::{Scheme, Tok, Variant, ZeroTemperatureRange};
use material_colors::color::Argb;
use material_colors::hct::Hct;

pub struct TermOpts {
    pub harmony: f64,
    pub harmonize_threshold: f64,
    pub term_fg_boost: f64,
    pub blend_bg_fg: bool,
}

impl Default for TermOpts {
    fn default() -> Self {
        TermOpts {
            harmony: 0.8,
            harmonize_threshold: 100.0,
            term_fg_boost: 0.35,
            blend_bg_fg: false,
        }
    }
}

fn hex(a: Argb) -> String {
    format!("#{:02X}{:02X}{:02X}", a.red, a.green, a.blue)
}

pub fn parse_hex(s: &str) -> Option<Argb> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Argb::new(255, (v >> 16) as u8, (v >> 8) as u8, v as u8))
}

fn sanitize(d: f64) -> f64 {
    d.rem_euclid(360.0)
}

fn harmonize(design: Argb, source: Argb, threshold: f64, harmony: f64) -> Argb {
    let from = Hct::new(design);
    let to = Hct::new(source);
    let (fh, th) = (from.get_hue(), to.get_hue());
    let diff = 180.0 - ((fh - th).abs() - 180.0).abs();
    let rot = (diff * harmony).min(threshold);
    let dir = if sanitize(th - fh) <= 180.0 {
        1.0
    } else {
        -1.0
    };
    Argb::from(Hct::from(
        sanitize(fh + rot * dir),
        from.get_chroma(),
        from.get_tone(),
    ))
}

fn boost(a: Argb, chroma: f64, tone: f64) -> Argb {
    let h = Hct::new(a);
    Argb::from(Hct::from(
        h.get_hue(),
        h.get_chroma() * chroma,
        h.get_tone() * tone,
    ))
}

/// `term` is the parsed `terminal/scheme-base.json` (`None` = no `--termscheme`).
pub fn generate(
    seed: Argb,
    scheme_name: &str,
    dark: bool,
    term: Option<&serde_json::Value>,
    opts: &TermOpts,
) -> Result<String, ZeroTemperatureRange> {
    let scheme = Scheme::new(
        Hct::new(seed),
        Variant::from_scheme_name(scheme_name),
        dark,
        0.0,
    )?;
    let mut out = String::with_capacity(4096);
    let py_bool = |b: bool| if b { "True" } else { "False" };
    out += &format!("$darkmode: {};\n$transparent: False;\n", py_bool(dark));
    let mut colors: Vec<(&str, String)> = Tok::ALL
        .iter()
        .map(|&t| (t.name(), hex(scheme.argb(t))))
        .collect();
    let success: [(&str, &str); 4] = if dark {
        [
            ("success", "#B5CCBA"),
            ("onSuccess", "#213528"),
            ("successContainer", "#374B3E"),
            ("onSuccessContainer", "#D1E9D6"),
        ]
    } else {
        [
            ("success", "#4F6354"),
            ("onSuccess", "#FFFFFF"),
            ("successContainer", "#D1E8D5"),
            ("onSuccessContainer", "#0C1F13"),
        ]
    };
    colors.extend(success.iter().map(|(k, v)| (*k, v.to_string())));
    let get = |name: &str| {
        colors
            .iter()
            .find(|(k, _)| *k == name)
            .and_then(|(_, v)| parse_hex(v))
            .unwrap()
    };
    let mut terms = Vec::new();
    if let Some(src) = term
        .and_then(|t| t.get(if dark { "dark" } else { "light" }))
        .and_then(|v| v.as_object())
    {
        let primary_key = get("primary_paletteKeyColor");
        for (name, val) in src {
            let Some(v) = val.as_str().and_then(parse_hex) else {
                continue;
            };
            let c = if opts.blend_bg_fg && name == "term0" {
                boost(get("surfaceContainerLow"), 1.2, 0.95)
            } else if opts.blend_bg_fg && name == "term15" {
                boost(get("onSurface"), 3.0, 1.0)
            } else {
                let h = harmonize(v, primary_key, opts.harmonize_threshold, opts.harmony);
                boost(
                    h,
                    1.0,
                    1.0 + opts.term_fg_boost * if dark { 1.0 } else { -1.0 },
                )
            };
            terms.push((name.clone(), hex(c)));
        }
    }
    for (k, v) in &colors {
        out += &format!("${k}: {v};\n");
    }
    for (k, v) in &terms {
        out += &format!("${k}: {v};\n");
    }
    Ok(out)
}
