//! `parse_status FILE...`: for each QML document its name, the status Qt's `Component` has
//! after parsing it (1 Ready, 3 Error, 0 Null) and its `errorString()` as JSON, as
//! `tools/gen/qqmljs/fuzz/probe.py` prints them for Qt.

fn main() {
    for file in std::env::args().skip(1) {
        let path = std::path::Path::new(&file);
        let name = path.file_name().unwrap().to_string_lossy();
        let src = std::fs::read_to_string(path).unwrap();
        let (status, text) = match qml::parse(&src) {
            Ok(_) => (1, String::new()),
            Err(e) if e.silent => (0, String::new()),
            Err(e) => (3, e.under(&name) + "\n"),
        };
        let stem = name.strip_suffix(".qml").unwrap_or(&name);
        println!("{stem} {status} {}", serde_json::to_string(&text).unwrap());
    }
}
