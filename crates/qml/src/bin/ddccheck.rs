//! `ddccheck [STEP]`: what the native Brightness service sees over DDC/CI (ddc-i2c), for
//! comparison with `ddcutil detect --brief` and `ddcutil -b N getvcp 10 --brief`.
fn main() {
    let step = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(0);
    for l in qml::brightness::ddc_probe(step) {
        println!("{l}");
    }
}
