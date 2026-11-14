//! One part of the compiled QML tree: the modules `ricompiled-gen` assigned to this crate.
#[path = "../src/abi.rs"]
pub mod abi;
/// Runtime helpers of generated code (`rigen::PRELUDE`), one copy shared by every part.
pub use ricompiled_rt as rt;
ricompiled_gen::include_gen!(env!("CARGO_PKG_NAME"), ".rs");
