// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

pub mod areaalloc;
pub mod blur;
pub mod canvas;
pub mod cosmetic;
pub mod dftext;
mod emoji;
mod emoji_data;
pub mod gradient;
mod hb_unicode;
pub mod image;
pub mod layer;
pub mod multieffect;
pub mod painter;
pub mod pathsimplify;
pub mod qdistancefield;
pub mod qmath;
pub mod raster;
pub mod rect;
pub mod shadow;
pub mod shape;
mod sine_table;
pub mod svg;
pub mod text;
pub mod triangulate;
