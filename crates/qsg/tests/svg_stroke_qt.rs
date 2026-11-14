//! SVG strokes (`<line>`, stroke-linecap, -linejoin, -miterlimit, -dasharray, -dashoffset)
//! against QtSvg itself: tools/oracle/svg_oracle.py renders the same files with QImageReader, the
//! reader Qt Quick's Image and QIcon use.
use std::path::{Path, PathBuf};

/// The PySide6 of the testenv image (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

struct Case {
    name: String,
    file: PathBuf,
    w: usize,
    h: usize,
}

/// Qt's pixels of every case, premultiplied RGBA; `None` (reported) without the oracle.
fn qt_render(dir: &Path, cases: &[Case]) -> Option<Vec<Vec<[u8; 4]>>> {
    if !Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return None;
    }
    let list: String = cases
        .iter()
        .map(|c| format!("{} {} {}\n", c.w, c.h, c.file.display()))
        .collect();
    let (list_p, out_p) = (dir.join("list"), dir.join("qt.rgba"));
    std::fs::write(&list_p, list).unwrap();
    let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/oracle/svg_oracle.py");
    let st = std::process::Command::new(PYTHON)
        .arg(oracle)
        .args([&out_p, &list_p])
        .env("QT_QPA_PLATFORM", "offscreen")
        .status()
        .unwrap();
    assert!(st.success(), "svg_oracle.py failed");
    let raw = std::fs::read(&out_p).unwrap();
    let mut at = 0;
    let out = cases
        .iter()
        .map(|c| {
            let n = c.w * c.h * 4;
            let px = raw[at..at + n].as_chunks::<4>().0.to_vec();
            at += n;
            px
        })
        .collect();
    assert_eq!(at, raw.len());
    Some(out)
}

/// (pixels that differ, largest channel difference) between Qt's render and ours.
fn compare(c: &Case, qt: &[[u8; 4]]) -> (usize, u8) {
    let doc = qsg::svg::Doc::load(c.file.to_str().unwrap()).unwrap();
    let img = doc.render(c.w, c.h).unwrap();
    let mut n = 0;
    let mut max = 0;
    for (a, b) in img.px.iter().zip(qt) {
        let d = (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap();
        n += (d > 0) as usize;
        max = max.max(d);
    }
    (n, max)
}

const SHAPES: &[(&str, &str)] = &[
    // A pen under the node's opacity: thinner than a pixel, wide, separate lines, with a
    // stroke opacity and a translucent colour, next to a fill.
    (
        "opacity-thin-stroke",
        r##"<rect x="4.4" y="5.3" width="22" height="19" rx="1" opacity="0.9" fill="none" stroke="#e2e2e2" stroke-width="0.18"/>"##,
    ),
    (
        "opacity-wide-stroke",
        r##"<path d="M5 26 L16 6 L27 26" opacity="0.7" fill="none" stroke="#c03" stroke-width="3"/>"##,
    ),
    (
        "opacity-lines",
        r##"<g opacity="0.45" stroke="#06c" stroke-width="2"><line x1="5" y1="5" x2="27" y2="5"/></g><line x1="5" y1="12" x2="27" y2="26" opacity="0.3" stroke="#080" stroke-width="2.5"/>"##,
    ),
    (
        "opacity-stroke-opacity",
        r##"<path d="M4 24 L28 16 L4 12" opacity="0.6" fill="none" stroke="#1a5fb4" stroke-opacity="0.5" stroke-width="3"/><path d="M4 4 H28" opacity="0.35" stroke="#1a5fb4" stroke-opacity="0.7" stroke-width="0.4"/>"##,
    ),
    (
        "opacity-fill-and-stroke",
        r##"<rect x="6" y="6" width="20" height="18" rx="3" opacity="0.8" fill="#fc0" stroke="#000" stroke-width="0.3"/>"##,
    ),
    (
        "line",
        r##"<line x1="4" y1="6" x2="27" y2="21" stroke="#c03" stroke-width="3"/>"##,
    ),
    (
        "line-in-group",
        r##"<g stroke="#06c" stroke-width="2" stroke-linecap="round"><line x1="5" y1="5" x2="27" y2="5"/><line x1="5" y1="12" x2="27" y2="26"/></g>"##,
    ),
    (
        "cap-butt",
        r##"<path d="M8 8 L24 20" fill="none" stroke="#000" stroke-width="5" stroke-linecap="butt"/>"##,
    ),
    (
        "cap-square",
        r##"<path d="M8 8 L24 20" fill="none" stroke="#000" stroke-width="5" stroke-linecap="square"/>"##,
    ),
    (
        "cap-round",
        r##"<path d="M8 8 L24 20" fill="none" stroke="#000" stroke-width="5" stroke-linecap="round"/>"##,
    ),
    (
        "cap-style",
        r##"<path d="M6 16 H26" style="fill:none;stroke:#080;stroke-width:6;stroke-linecap:round"/>"##,
    ),
    (
        "join-default",
        r##"<path d="M5 26 L16 6 L27 26" fill="none" stroke="#000" stroke-width="4"/>"##,
    ),
    (
        "join-miter",
        r##"<path d="M5 26 L16 6 L27 26" fill="none" stroke="#000" stroke-width="4" stroke-linejoin="miter"/>"##,
    ),
    (
        "join-round",
        r##"<path d="M5 26 L16 6 L27 26" fill="none" stroke="#000" stroke-width="4" stroke-linejoin="round"/>"##,
    ),
    (
        "join-bevel",
        r##"<path d="M5 26 L16 6 L27 26" fill="none" stroke="#000" stroke-width="4" stroke-linejoin="bevel"/>"##,
    ),
    (
        "miter-sharp-default",
        r##"<path d="M4 24 L28 16 L4 12" fill="none" stroke="#000" stroke-width="3"/>"##,
    ),
    (
        "miterlimit-1",
        r##"<path d="M5 26 L16 6 L27 26" fill="none" stroke="#000" stroke-width="4" stroke-miterlimit="1"/>"##,
    ),
    (
        "miterlimit-2",
        r##"<path d="M4 24 L28 16 L4 12" fill="none" stroke="#000" stroke-width="3" stroke-miterlimit="2"/>"##,
    ),
    (
        "miterlimit-10",
        r##"<path d="M4 24 L28 16 L4 12" fill="none" stroke="#000" stroke-width="3" stroke-miterlimit="10"/>"##,
    ),
    (
        "closed-round",
        r##"<rect x="6" y="6" width="20" height="18" fill="#fc0" stroke="#000" stroke-width="4" stroke-linejoin="round"/>"##,
    ),
    (
        "closed-bevel",
        r##"<polygon points="16,4 28,26 4,26" fill="none" stroke="#000" stroke-width="4" stroke-linejoin="bevel"/>"##,
    ),
    (
        "dash",
        r##"<path d="M3 16 H29" fill="none" stroke="#000" stroke-width="2" stroke-dasharray="4,2"/>"##,
    ),
    (
        "dash-spaces",
        r##"<path d="M3 16 H29" fill="none" stroke="#000" stroke-width="2" stroke-dasharray="4 2 1 2"/>"##,
    ),
    (
        "dash-odd",
        r##"<path d="M3 16 H29" fill="none" stroke="#000" stroke-width="2" stroke-dasharray="5"/>"##,
    ),
    (
        "dash-offset",
        r##"<path d="M3 16 H29" fill="none" stroke="#000" stroke-width="2" stroke-dasharray="4,2" stroke-dashoffset="3"/>"##,
    ),
    (
        "dash-round",
        r##"<path d="M4 16 H28" fill="none" stroke="#000" stroke-width="3" stroke-dasharray="0.1,6" stroke-linecap="round"/>"##,
    ),
    (
        "dash-square",
        r##"<path d="M4 8 L28 24" fill="none" stroke="#000" stroke-width="3" stroke-dasharray="3,6" stroke-linecap="square"/>"##,
    ),
    (
        "dash-none",
        r##"<g stroke-dasharray="4,2"><path d="M3 16 H29" fill="none" stroke="#000" stroke-width="2" stroke-dasharray="none"/></g>"##,
    ),
    (
        "dash-zero",
        r##"<path d="M3 16 H29" fill="none" stroke="#000" stroke-width="2" stroke-dasharray="0,0"/>"##,
    ),
    (
        "dash-inherited-width",
        r##"<g stroke-width="4" stroke="#000"><path d="M3 16 H29" fill="none" stroke-dasharray="4,2"/></g>"##,
    ),
    (
        "dash-inherited-pattern",
        r##"<g stroke-dasharray="4,2" stroke="#000"><path d="M3 10 H29" fill="none" stroke-width="2"/><path d="M3 22 H29" fill="none" stroke-width="4"/></g>"##,
    ),
    (
        "dash-curve",
        r##"<circle cx="16" cy="16" r="11" fill="none" stroke="#909" stroke-width="2" stroke-dasharray="3,2"/>"##,
    ),
    (
        "dash-scaled",
        r##"<g transform="scale(2)"><path d="M2 8 H14" fill="none" stroke="#000" stroke-width="1" stroke-dasharray="2,1"/></g>"##,
    ),
    (
        "polyline-round",
        r##"<polyline points="4,26 10,8 16,24 22,8 28,26" fill="none" stroke="#036" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>"##,
    ),
];

/// What QtSvg and the raster engine do beyond strokes: gradients (an opaque one composed as
/// Source, a vertical one a colour a row), filters (a blur under a rotated painter copied and
/// drawn back as an image), masks, group opacity, text, images, and rectangles, ellipses and
/// arcs under a transform.
const PAINTED: &[(&str, &str)] = &[
    (
        "gradient-opaque-rounded",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><linearGradient id="g" xlink:href="#o" gradientUnits="userSpaceOnUse" x1="3" y1="5" x2="29" y2="27"/><rect x="3.3" y="4.6" width="25" height="22" rx="3.5" fill="url(#g)"/>"##,
    ),
    (
        "gradient-opaque-on-colour",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><linearGradient id="g" xlink:href="#o" gradientUnits="userSpaceOnUse" x1="4" y1="16" x2="28" y2="16"/><rect width="32" height="32" fill="#f6d32d"/><circle cx="16" cy="16" r="11.3" fill="url(#g)"/>"##,
    ),
    (
        "gradient-vertical",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><linearGradient id="g" xlink:href="#o" gradientUnits="userSpaceOnUse" x1="0" y1="4" x2="0" y2="28"/><rect x="3.3" y="4.6" width="25" height="22" rx="3.5" fill="url(#g)"/>"##,
    ),
    (
        "gradient-vertical-box",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><linearGradient id="g" xlink:href="#o" x1="0" y1="0" x2="0" y2="1"/><path d="M4 28 L16 3 L28 28 Z" fill="url(#g)"/>"##,
    ),
    (
        "gradient-vertical-rotated",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><linearGradient id="g" xlink:href="#o" gradientUnits="userSpaceOnUse" x1="0" y1="4" x2="0" y2="28"/><g transform="rotate(20 16 16)"><rect x="6" y="7" width="20" height="18" rx="3" fill="url(#g)"/></g>"##,
    ),
    (
        "gradient-alpha",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><linearGradient id="g" xlink:href="#a" gradientUnits="userSpaceOnUse" x1="3" y1="5" x2="29" y2="27"/><rect width="32" height="32" fill="#99c1f1"/><rect x="3.3" y="4.6" width="25" height="22" rx="3.5" fill="url(#g)"/>"##,
    ),
    (
        "gradient-opacity",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><linearGradient id="g" xlink:href="#o" gradientUnits="userSpaceOnUse" x1="3" y1="5" x2="29" y2="27"/><rect x="3.3" y="4.6" width="25" height="22" rx="3.5" fill="url(#g)" fill-opacity="0.6"/>"##,
    ),
    (
        "gradient-stroke",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><linearGradient id="g" xlink:href="#o" gradientUnits="userSpaceOnUse" x1="3" y1="5" x2="29" y2="27"/><path d="M5 26 L16 6 L27 26" fill="none" stroke="url(#g)" stroke-width="4" stroke-linejoin="round"/>"##,
    ),
    (
        "gradient-radial",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><radialGradient id="g" xlink:href="#o" gradientUnits="userSpaceOnUse" cx="14" cy="13" r="13"/><circle cx="16" cy="16" r="12.5" fill="url(#g)"/>"##,
    ),
    (
        "gradient-radial-focal",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><radialGradient id="g" xlink:href="#a" cx="0.5" cy="0.5" r="0.5" fx="0.3" fy="0.25"/><rect x="3" y="3" width="26" height="26" fill="url(#g)"/>"##,
    ),
    (
        "gradient-transform",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><linearGradient id="g" xlink:href="#o" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="10" y2="0" gradientTransform="matrix(1.5 0.8 -0.8 1.5 8 2)" spreadMethod="reflect"/><rect x="3" y="3" width="26" height="26" fill="url(#g)"/>"##,
    ),
    (
        "blur",
        r##"<filter id="f"><feGaussianBlur stdDeviation="1.3"/></filter><circle cx="16" cy="16" r="8" fill="#c01c28" filter="url(#f)"/>"##,
    ),
    (
        "blur-region",
        r##"<filter id="f" x="-0.1" y="-0.1" width="1.2" height="1.2"><feGaussianBlur stdDeviation="2 0.7"/></filter><rect x="8" y="9" width="16" height="13" fill="#1a5fb4" filter="url(#f)"/>"##,
    ),
    (
        "blur-rotated",
        r##"<filter id="f"><feGaussianBlur stdDeviation="1.2"/></filter><g transform="matrix(0.7071,-0.7071,0.7071,0.7071,-6,16)"><rect x="9" y="10" width="15" height="11" fill="#613583" filter="url(#f)"/></g>"##,
    ),
    (
        "blur-rotated-small",
        r##"<filter id="f"><feGaussianBlur stdDeviation="0.4"/></filter><g transform="rotate(25 16 16)"><rect x="14" y="14" width="4" height="3" fill="#613583" filter="url(#f)"/></g>"##,
    ),
    (
        "blur-rotated-dash-opacity",
        r##"<filter id="f"><feGaussianBlur stdDeviation="0.6"/></filter><g transform="rotate(-15 16 16)"><path d="M5 20 C10 6 22 6 27 20" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-dasharray="3,3" opacity="0.59" filter="url(#f)"/></g>"##,
    ),
    (
        "blur-mirrored",
        r##"<filter id="f"><feGaussianBlur stdDeviation="1"/></filter><g transform="matrix(-1 0 0 1 32 0)"><path d="M6 6 H20 V26 Z" fill="#e66100" filter="url(#f)"/></g>"##,
    ),
    (
        "filter-shadow",
        r##"<filter id="f" x="-0.3" y="-0.3" width="1.6" height="1.6"><feOffset in="SourceAlpha" dx="2" dy="2" result="o"/><feGaussianBlur in="o" stdDeviation="1" result="b"/><feFlood flood-color="#241f31" flood-opacity="0.7" result="c"/><feComposite in="c" in2="b" operator="in" result="s"/><feMerge><feMergeNode in="s"/><feMergeNode in="SourceGraphic"/></feMerge></filter><rect x="7" y="7" width="15" height="15" rx="2" fill="#33d17a" filter="url(#f)"/>"##,
    ),
    (
        "filter-matrix",
        r##"<filter id="f"><feColorMatrix type="saturate" values="0.3"/></filter><g filter="url(#f)"><rect x="4" y="4" width="14" height="14" fill="#e01b24"/><circle cx="20" cy="20" r="8" fill="#1c71d8" fill-opacity="0.7"/></g>"##,
    ),
    (
        "mask",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><linearGradient id="g" xlink:href="#a" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="32" y2="0"/><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="32" height="32"><rect width="32" height="32" fill="#fff"/><circle cx="16" cy="16" r="6" fill="#000"/></mask><rect x="3" y="3" width="26" height="26" fill="#986a44" mask="url(#m)"/>"##,
    ),
    (
        "group-opacity",
        r##"<g opacity="0.5"><rect x="4" y="4" width="16" height="16" fill="#e01b24"/><circle cx="20" cy="20" r="8" fill="#1c71d8"/></g>"##,
    ),
    (
        "group-opacity-nested",
        r##"<rect width="32" height="32" fill="#deddda"/><g opacity="0.7" transform="rotate(10 16 16)"><rect x="4" y="4" width="16" height="16" fill="#e01b24"/><g opacity="0.4"><circle cx="20" cy="20" r="8" fill="#1c71d8"/><path d="M4 28 H28" stroke="#000" stroke-width="2"/></g></g>"##,
    ),
    (
        "text",
        r##"<text x="2" y="22" font-size="18" fill="#241f31">Qt</text>"##,
    ),
    (
        "text-family-weight",
        r##"<text x="2" y="20" style="font-family:DejaVu Sans;font-weight:bold;font-size:11px;fill:#1a5fb4">Abg</text>"##,
    ),
    (
        "text-italic-serif",
        r##"<text x="2" y="20" style="font-family:serif;font-style:italic;font-size:12px">fyi</text>"##,
    ),
    (
        "text-anchor-middle",
        r##"<text x="16" y="20" font-size="10" text-anchor="middle">mid</text>"##,
    ),
    (
        "text-anchor-end",
        r##"<text x="30" y="20" font-size="10" text-anchor="end">end</text>"##,
    ),
    (
        "text-tspan",
        r##"<text x="2" y="14" font-size="9" fill="#a51d2d">ab<tspan fill="#26a269" font-weight="bold">cd</tspan><tspan x="2" y="26" font-size="12">ef</tspan></text>"##,
    ),
    (
        "text-space",
        r##"<text x="2" y="20" font-size="9">  a   b  <tspan> c</tspan></text>"##,
    ),
    (
        "text-stroke",
        r##"<text x="3" y="24" style="font-size:24px;fill:#f6d32d;stroke:#241f31;stroke-width:1.2;stroke-linejoin:round">K</text>"##,
    ),
    (
        "text-stroke-tspan",
        r##"<text x="3" y="24" style="font-size:24px;fill:#555;stroke:#555;stroke-width:1.7;stroke-linecap:round"><tspan style="fill:#555;stroke:#555;stroke-width:1.7" x="3" y="24">K</tspan></text>"##,
    ),
    (
        "text-transformed",
        r##"<g transform="matrix(1.3 0.4 -0.3 1.1 6 -2)"><text x="2" y="20" font-size="13" fill="#63452c">Rs</text></g>"##,
    ),
    (
        "text-gradient",
        r##"<linearGradient id="o"><stop offset="0" stop-color="#e01b24"/><stop offset="1" stop-color="#1c71d8"/></linearGradient><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#26a269" stop-opacity="0"/></linearGradient><linearGradient id="g" xlink:href="#o" gradientUnits="userSpaceOnUse" x1="2" y1="0" x2="30" y2="0"/><text x="2" y="24" font-size="22" font-weight="bold" fill="url(#g)">W</text>"##,
    ),
    (
        "text-blur",
        r##"<filter id="f"><feGaussianBlur stdDeviation="0.5"/></filter><text x="3" y="24" font-size="22" fill="#555" filter="url(#f)">K</text>"##,
    ),
    (
        "text-opacity",
        r##"<rect width="32" height="32" fill="#99c1f1"/><text x="3" y="24" font-size="22" fill="#000" fill-opacity="0.5" opacity="0.8">e</text>"##,
    ),
    (
        "image",
        r##"<rect width="32" height="32" fill="#356"/><image x="4" y="6" width="24" height="20" xlink:href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAYAAAAFCAYAAABmWJ3mAAAAdElEQVR42g2MoQ3DUAxED5R2gSwQHn1o0gUyhUEWqGRodYiPMoY3KDHsKMafXA1Od9LTOwDwA6B2Zu/sXh1AwEOeVNl8ys6UwSUvB6yBba42OO1k2uXLPm1EX8VOjZMz3p5xc8W3QbVRg1qXz7qZ9eOqh/8BYTs8Qf3M3JsAAAAASUVORK5CYII="/>"##,
    ),
    (
        "image-rotated",
        r##"<rect width="32" height="32" fill="#356"/><image transform="translate(14 3) rotate(30)" width="18" height="15" xlink:href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAYAAAAFCAYAAABmWJ3mAAAAdElEQVR42g2MoQ3DUAxED5R2gSwQHn1o0gUyhUEWqGRodYiPMoY3KDHsKMafXA1Od9LTOwDwA6B2Zu/sXh1AwEOeVNl8ys6UwSUvB6yBba42OO1k2uXLPm1EX8VOjZMz3p5xc8W3QbVRg1qXz7qZ9eOqh/8BYTs8Qf3M3JsAAAAASUVORK5CYII="/>"##,
    ),
    (
        "image-down-opacity",
        r##"<image x="10.3" y="12.6" width="4.5" height="3.2" opacity="0.6" xlink:href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAYAAAAFCAYAAABmWJ3mAAAAdElEQVR42g2MoQ3DUAxED5R2gSwQHn1o0gUyhUEWqGRodYiPMoY3KDHsKMafXA1Od9LTOwDwA6B2Zu/sXh1AwEOeVNl8ys6UwSUvB6yBba42OO1k2uXLPm1EX8VOjZMz3p5xc8W3QbVRg1qXz7qZ9eOqh/8BYTs8Qf3M3JsAAAAASUVORK5CYII="/>"##,
    ),
    (
        "rect-rotated",
        r##"<g transform="rotate(33 16 16)"><rect x="8" y="10" width="16" height="12" fill="#c64600"/></g><g transform="matrix(1 0.3 0.5 1 -6 -4)"><rect x="18" y="20" width="9" height="7" fill="#1a5fb4"/></g>"##,
    ),
    (
        "rect-rounded",
        r##"<rect x="2.5" y="3.5" width="27" height="12" rx="6" ry="3" fill="#77767b"/><rect x="4" y="18" width="24" height="11" rx="30" fill="none" stroke="#000"/><rect x="10" y="20" width="12" height="7" rx="0" ry="3" fill="#e5a50a"/>"##,
    ),
    (
        "ellipse-rotated",
        r##"<g transform="translate(-384.57143,-515.798)"><ellipse style="fill:none;stroke:#3d3846;stroke-width:1.00346744" transform="matrix(-0.74741636,-0.66435592,-0.74741636,0.66435592,0,0)" cx="-668.2066" cy="132.26517" rx="10.365829" ry="3.5"/></g>"##,
    ),
    (
        "path-arcs-closed",
        r##"<path d="m18 4c-2.2 0-4 1.8-4 4v13.5a6 6 0 0 0-2 4.5 6 6 0 0 0 6 6 6 6 0 0 0 6-6 6 6 0 0 0-2-4.5v-13.5c0-2.2-1.8-4-4-4zm0 2c1.1 0 2 0.9 2 2v14.5a4 3.9 0 0 1 2 3.4 4 3.9 0 0 1-4 3.9 4 3.9 0 0 1-4-3.9 4 3.9 0 0 1 2-3.4v-14.5c0-1.1 0.9-2 2-2z" transform="scale(0.9)"/>"##,
    ),
];

/// The value after `key` (`key="v"`, `key:v`) at each of its occurrences.
fn values<'a>(src: &'a str, key: &str) -> Vec<&'a str> {
    src.match_indices(key)
        .map(|(i, _)| {
            let v = src[i + key.len()..].trim_start_matches([':', '=', '"', '\'', ' ']);
            v[..v.find([';', '"', '\'']).unwrap_or(v.len())].trim()
        })
        .collect()
}

/// A file that asks for something other than the default pen (butt caps, miter joins with
/// limit 4, solid) or holds a `<line>`.
fn uses_stroke_features(src: &str) -> bool {
    src.contains("<line ")
        || values(src, "stroke-linecap").iter().any(|v| *v != "butt")
        || values(src, "stroke-linejoin").iter().any(|v| *v != "miter")
        || values(src, "stroke-dasharray").iter().any(|v| *v != "none")
        || values(src, "stroke-miterlimit")
            .iter()
            .any(|v| v.parse::<f64>().ok() != Some(4.0))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut ents: Vec<_> = rd.flatten().collect();
    ents.sort_by_key(|e| e.file_name());
    for e in ents {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(&p, out);
        } else if ft.is_file() && p.extension().is_some_and(|x| x == "svg") {
            out.push(p);
        }
    }
}

/// Prints how far the cases are from Qt and returns those whose largest channel difference is
/// over `limit`.
fn report(what: &str, cases: &[Case], qt: &[Vec<[u8; 4]>], limit: u8) -> Vec<String> {
    let mut differ = vec![];
    let mut hist = [0usize; 5];
    let mut dump = std::env::var_os("SVGQT_DUMP").map(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .unwrap()
    });
    for (c, q) in cases.iter().zip(qt) {
        let (n, max) = compare(c, q);
        let b = match max {
            0 => 0,
            1 => 1,
            2..=3 => 2,
            4..=16 => 3,
            _ => 4,
        };
        hist[b] += 1;
        if let Some(d) = dump.as_mut() {
            use std::io::Write;
            writeln!(d, "{}\t{}x{}\t{n}\t{max}", c.name, c.w, c.h).unwrap();
        }
        if max > limit {
            differ.push(format!(
                "{} {}x{}: {n} px differ, max {max}",
                c.name, c.w, c.h
            ));
        }
    }
    elog::eprintln!(
        "{what}: {} cases; max channel difference 0: {}, 1: {}, 2-3: {}, 4-16: {}, >16: {}",
        cases.len(),
        hist[0],
        hist[1],
        hist[2],
        hist[3],
        hist[4]
    );
    for d in differ.iter().take(40) {
        elog::eprintln!("  {d}");
    }
    differ
}

#[test]
fn stroke_shapes_match_qt() {
    let dir = tempfile::tempdir().unwrap();
    let mut cases = vec![];
    for (name, body) in SHAPES {
        let file = dir.path().join(format!("{name}.svg"));
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">{body}</svg>"##
        );
        std::fs::write(&file, svg).unwrap();
        for s in [32, 48, 96] {
            cases.push(Case {
                name: name.to_string(),
                file: file.clone(),
                w: s,
                h: s,
            });
        }
    }
    let Some(qt) = qt_render(dir.path(), &cases) else {
        return;
    };
    let differ = report("stroke shapes", &cases, &qt, 0);
    assert!(differ.is_empty(), "{differ:#?}");
}

#[test]
fn painted_shapes_match_qt() {
    let dir = tempfile::tempdir().unwrap();
    let mut cases = vec![];
    for (name, body) in PAINTED {
        let file = dir.path().join(format!("{name}.svg"));
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="32" height="32" viewBox="0 0 32 32">{body}</svg>"##
        );
        std::fs::write(&file, svg).unwrap();
        for s in [32, 48, 96] {
            cases.push(Case {
                name: name.to_string(),
                file: file.clone(),
                w: s,
                h: s,
            });
        }
    }
    let Some(qt) = qt_render(dir.path(), &cases) else {
        return;
    };
    let differ = report("painted shapes", &cases, &qt, 1);
    assert!(differ.is_empty(), "{differ:#?}");
}

#[test]
fn theme_icons_with_stroke_features_match_qt() {
    let mut files = vec![];
    walk(Path::new("/usr/share/icons"), &mut files);
    let mut cases = vec![];
    for file in files {
        let Ok(src) = std::fs::read_to_string(&file) else {
            continue;
        };
        if !uses_stroke_features(&src) {
            continue;
        }
        let Ok(doc) = qsg::svg::Doc::load(file.to_str().unwrap()) else {
            continue;
        };
        let (w, h) = doc.default_size();
        if w < 1 || h < 1 || w > 256 || h > 256 {
            continue;
        }
        cases.push(Case {
            name: file.display().to_string(),
            file,
            w: w as usize,
            h: h as usize,
        });
    }
    if cases.is_empty() {
        testenv::equiv_missing("icon themes under /usr/share/icons");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let Some(qt) = qt_render(dir.path(), &cases) else {
        return;
    };
    let differ = report("theme icons with stroke features", &cases, &qt, 3);
    assert!(cases.len() > 1000, "{} icons", cases.len());
    assert!(
        differ.is_empty(),
        "{} icons differ: {differ:?}",
        differ.len()
    );
}

/// Every SVG file under `root` against Qt, at its own size and as a square of each of
/// `squares`: the renders further from it than 3 in any channel of any pixel, and how many
/// renders there were. `None` without the files or the oracle.
fn every_svg_against_qt(
    what: &str,
    root: &Path,
    squares: &[usize],
) -> Option<(Vec<String>, usize)> {
    let mut files = vec![];
    walk(root, &mut files);
    let mut cases = vec![];
    for file in files {
        let Ok(doc) = qsg::svg::Doc::load(file.to_str().unwrap()) else {
            elog::eprintln!("  not loaded: {}", file.display());
            continue;
        };
        let (w, h) = doc.default_size();
        if w < 1 || h < 1 {
            elog::eprintln!("  size {w}x{h}: {}", file.display());
            continue;
        }
        for &s in squares {
            cases.push(Case {
                name: file.display().to_string(),
                file: file.clone(),
                w: s,
                h: s,
            });
        }
        cases.push(Case {
            name: file.display().to_string(),
            file,
            w: w as usize,
            h: h as usize,
        });
    }
    if cases.is_empty() {
        testenv::equiv_missing(format_args!("SVG files under {}", root.display()));
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    let qt = qt_render(dir.path(), &cases)?;
    Some((report(what, &cases, &qt, 3), cases.len()))
}

/// Every SVG file of the image's icon themes. Before filters, masks, gradients, text and the
/// rest were drawn Qt's way 681 of these icons were further from Qt than 3.
#[test]
fn all_theme_icons_match_qt() {
    let Some((differ, n)) =
        every_svg_against_qt("all theme icons", Path::new("/usr/share/icons"), &[])
    else {
        return;
    };
    assert!(n > 16000, "{n} icons");
    assert!(
        differ.is_empty(),
        "{} icons differ: {differ:?}",
        differ.len()
    );
}

/// Every SVG file the shell's own QML tree ships, also at the sizes icons are asked for.
#[test]
fn the_original_assets_match_qt() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let Some((differ, n)) = every_svg_against_qt("QML assets", &root, &[16, 24, 64]) else {
        return;
    };
    assert!(n > 800, "{n} renders");
    assert!(
        differ.is_empty(),
        "{} assets differ: {differ:?}",
        differ.len()
    );
}

/// Waffle's task view button (`TaskViewButton.qml`): a `Kirigami.Icon` of 26 at the monitor
/// scales 1, 1.5, 2 and 3, square and in the file's own proportions. Its light rectangle has a
/// pen of 0.18 under an opacity of 0.9: the pen's colour was 1 of 255 off Qt's on every pixel
/// of the outline.
#[test]
fn task_view_icon_matches_qt() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml/assets/icons/fluent");
    let mut cases = vec![];
    for name in [
        "task-view-dark",
        "task-view-light",
        "task-view-pressed-dark",
        "task-view-pressed-light",
    ] {
        let file = root.join(format!("{name}.svg"));
        for s in [26, 39, 52, 78] {
            for h in [s, (s as f64 * 61.62962 / 63.999989).round() as usize] {
                cases.push(Case {
                    name: name.to_string(),
                    file: file.clone(),
                    w: s,
                    h,
                });
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let Some(qt) = qt_render(dir.path(), &cases) else {
        return;
    };
    let differ = report("task view icon", &cases, &qt, 0);
    assert!(differ.is_empty(), "{differ:#?}");
}
