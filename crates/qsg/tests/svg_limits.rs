//! Untrusted SVGs (themed, tray and notification icons) cannot crash or stall the renderer:
//! recursive masks, `<use>` fan-out and huge intrinsic sizes.

use qsg::svg::Doc;

fn doc(name: &str, body: &str) -> Doc {
    let dir = std::env::temp_dir().join(format!("qsg-svg-limits-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    let d = Doc::load(p.to_str().unwrap()).unwrap();
    std::fs::remove_file(&p).ok();
    d
}

/// A mask referenced from its own content is ignored there: the inner rect is drawn unmasked,
/// so the mask is opaque white and the outer rect shows in full.
#[test]
fn self_referencing_mask_terminates() {
    let d = doc(
        "mask_self.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><mask id="m"><rect width="8" height="8" fill="#fff" mask="url(#m)"/></mask><rect width="8" height="8" mask="url(#m)"/></svg>"##,
    );
    let img = d.render(8, 8).unwrap();
    assert!(
        img.px.iter().all(|p| *p == [0, 0, 0, 255]),
        "{:?}",
        img.px[0]
    );
}

#[test]
fn mask_cycle_terminates() {
    let d = doc(
        "mask_cycle.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><mask id="a"><rect width="8" height="8" fill="#fff" mask="url(#b)"/></mask><mask id="b"><rect width="8" height="8" fill="#fff" mask="url(#a)"/></mask><rect width="8" height="8" mask="url(#a)"/></svg>"##,
    );
    let img = d.render(8, 8).unwrap();
    assert_eq!(img.px[4 * 8 + 4], [0, 0, 0, 255]);
}

fn fanout(levels: usize, n: usize) -> String {
    let mut s = String::from(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="16" height="16"><defs><rect id="l0" width="1" height="1"/>"#,
    );
    for l in 1..=levels {
        s += &format!("<g id=\"l{l}\">");
        for _ in 0..n {
            s += &format!("<use xlink:href=\"#l{}\"/>", l - 1);
        }
        s += "</g>";
    }
    s + &format!("</defs><use xlink:href=\"#l{levels}\"/></svg>")
}

/// 6^10 = 60M rect draws from a 1.7 KB file: the render stops at the reference budget.
#[test]
fn use_fanout_is_bounded() {
    let d = doc("fanout.svg", &fanout(10, 6));
    assert!(d.render(16, 16).is_err(), "fan-out rendered to completion");
}

#[test]
fn small_use_fanout_still_renders() {
    let d = doc("fanout_small.svg", &fanout(4, 6));
    let img = d.render(16, 16).unwrap();
    assert_eq!(img.px[0], [0, 0, 0, 255]);
    assert_eq!(img.px[17], [0, 0, 0, 0]);
}

/// Rendering at the intrinsic size goes through the decode limit (a 40 GB target is refused).
#[test]
fn huge_intrinsic_size_is_refused() {
    let d = doc(
        "big.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100000" height="100000"/>"#,
    );
    let (w, h) = d.default_size();
    assert_eq!((w, h), (100000, 100000));
    let r = d.render(w as usize, h as usize);
    assert!(
        r.as_ref()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::OutOfMemory)
    );
}

#[test]
fn negative_intrinsic_size_is_empty() {
    let d = doc(
        "neg.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="-5" height="7"/>"#,
    );
    assert_eq!(d.default_size(), (0, 7));
}
