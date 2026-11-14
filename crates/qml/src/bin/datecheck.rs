//! Print Qt-style formatted 2024-02-01 13:05:09.007 local for each format arg (oracle comparison).
fn main() {
    let ms = {
        let mut it = qml::eval::Interp::default();
        let body =
            qml::ir::lower_program("return new Date(2024, 1, 1, 13, 5, 9, 7).getTime()").unwrap();
        let g = it.global.clone();
        it.run(&body, &g).ok().unwrap().to_number()
    };
    for f in std::env::args().skip(1) {
        println!("{}", qml::builtins::qt_format(ms, &f));
    }
}
