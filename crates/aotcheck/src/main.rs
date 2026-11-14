//! `aotcheck` without compiled code (`--backend interp` only); the ricompiled build
//! passes its registry for `--backend compiled`.

// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

fn main() {
    qml::debug::init_log();
    std::process::exit(aotcheck::run(std::env::args().skip(1).collect(), None));
}
