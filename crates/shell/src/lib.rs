//! Native shell: geometry + painting of runtime item trees, Wayland front-end.

// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

/// Whether the debug/tuning env var `$n` is set, read once per call site.
#[macro_export]
macro_rules! env_flag {
    ($n:literal) => {{
        static F: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *F.get_or_init(|| std::env::var_os($n).is_some())
    }};
}

pub mod canvas2d;
pub mod dmabuf;
pub mod gbm;
pub mod gpu;
pub mod heap;
pub mod hidpi;
pub mod hyprclose;
pub mod input;
pub mod ipc;
pub mod keys;
mod keysym;
pub mod layer;
pub mod line_break;
mod line_break_data;
pub mod movie;
pub mod output_heads;
pub mod panics;
pub mod pragmas;
pub mod region;
pub mod reload;
pub mod ricli;
pub mod rilog;
pub mod scene;
pub mod sched;
pub mod selection;
pub mod shutdown;
pub mod text_layout;
pub mod textmouse;
pub mod wlread;
