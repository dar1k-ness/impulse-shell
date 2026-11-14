//! switchwall <script dir> <args..>: the in-process port against the live session, as the
//! shell runs it for `execDetached([".../colors/switchwall.sh", ...])`.
use colorgen::switchwall::{self as sw, Paths};
use colorgen::sys::Sys;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let sys: Arc<dyn Sys> = Arc::new(colorgen::live::Live);
    let rc = sw::main(&a[1..], &Paths::from_env(a[0].clone().into()), &sys);
    std::process::exit(rc);
}
