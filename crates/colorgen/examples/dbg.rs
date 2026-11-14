use colorgen::mdc::*;
use material_colors::{color::Argb, hct::Hct};
fn main() {
    let s = Scheme::new(
        Hct::new(Argb::new(255, 0, 0, 0)),
        Variant::Content,
        true,
        0.0,
    )
    .unwrap();
    for t in [Tok::primaryContainer, Tok::onPrimaryContainer, Tok::primary] {
        println!("{:?} tone={} argb={}", t, s.tone(t), s.argb(t).to_hex());
    }
    println!("p {:?}", s.primary);
}
