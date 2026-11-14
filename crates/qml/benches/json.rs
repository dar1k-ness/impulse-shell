//! JSON.parse / JSON.stringify on Hyprland-sized input: `cargo bench -p qml --bench json`.
use criterion::{Criterion, criterion_group, criterion_main};
use qml::eval::Interp;
use std::hint::black_box;

/// `hyprctl -j clients` with 40 windows: hypr.rs parses it on every Hyprland event.
fn clients() -> String {
    let client = r#"{"address":"0x55d0c1a2b3c0","mapped":true,"hidden":false,"at":[1932,52],"size":[1896,1076],"workspace":{"id":3,"name":"3"},"floating":false,"pseudo":false,"monitor":1,"class":"google-chrome","title":"end-4 — Rust \u2014 Google Chrome","initialClass":"google-chrome","initialTitle":"New Tab","pid":12345,"xwayland":false,"pinned":false,"fullscreen":0,"fullscreenClient":0,"grouped":[],"tags":[],"swallowing":"0x0","focusHistoryID":2,"inhibitingIdle":false,"xdgTag":"","xdgDescription":"","contentType":"none"}"#;
    format!("[{}]", vec![client; 40].join(","))
}

fn json(c: &mut Criterion) {
    let text = clients();
    let v = qml::json::parse(&text).unwrap();
    let mut it = Interp::default();
    c.bench_function("json/parse", |b| {
        b.iter(|| qml::json::parse(black_box(&text)))
    });
    c.bench_function("json/stringify", |b| {
        b.iter(|| qml::json::stringify(&mut it, black_box(&v), ""))
    });
    c.bench_function("json/stringify_indent2", |b| {
        b.iter(|| qml::json::stringify(&mut it, black_box(&v), "  "))
    });
}

criterion_group!(benches, json);
criterion_main!(benches);
