//! Prints `QEasingCurve` values in the same format as the PySide6 reference dump.
use qml::easing::Easing;
fn main() {
    let curves: Vec<Vec<f64>> = std::env::args()
        .skip(1)
        .map(|a| a.split(',').map(|x| x.parse().unwrap()).collect())
        .collect();
    let emit = |label: String, e: &Easing| {
        for i in 0..=200 {
            let t = i as f64 / 200.0;
            println!("{label}\t{i}\t{:.17e}", e.value(t));
        }
    };
    for ty in 0..45 {
        emit(
            format!("t{ty}"),
            &Easing {
                ty,
                ..Default::default()
            },
        );
    }
    for (k, c) in curves.iter().enumerate() {
        let bezier = c.chunks(2).map(|p| (p[0], p[1])).collect();
        emit(
            format!("b{k}"),
            &Easing {
                ty: 45,
                bezier,
                ..Default::default()
            },
        );
    }
}
