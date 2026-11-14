//! The QML tree compiled by rigen at build time (build.rs), shared by the crates that compile it.
//!
//! The generated Rust is too big for one crate (rustc held ~7.5 GB for it), so its modules are
//! split over the `ricompiled-pNN` crates (`crates/ricompiled/parts`), which build in parallel
//! with a fraction of the memory each; `ricompiled` holds the registry over all of them.
include!(concat!(env!("OUT_DIR"), "/gen.rs"));
