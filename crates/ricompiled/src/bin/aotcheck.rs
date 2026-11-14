//! `aotcheck` with the QML code compiled into this crate as the `--backend compiled` candidate.
use std::path::Path;
use std::rc::Rc;

fn compiled(root: &Path) -> Rc<dyn qml::jsrt::Aot> {
    Rc::new(ricompiled::Compiled::new(root))
}

fn main() {
    qml::debug::init_log();
    std::process::exit(aotcheck::run(
        std::env::args().skip(1).collect(),
        Some(compiled),
    ));
}
