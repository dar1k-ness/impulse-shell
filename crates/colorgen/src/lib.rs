// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

pub mod apply;
pub mod code;
pub mod colorfulness;
pub mod fsutil;
pub mod hyprlive;
pub mod kde;
pub mod live;
pub mod mdc;
pub mod mg;
pub mod pillow;
pub mod quantize;
pub mod scss;
pub mod seed;
pub mod switchwall;
pub mod sys;
#[cfg(test)]
mod userenv_sites;
