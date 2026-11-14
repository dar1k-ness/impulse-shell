//! The QML engine of the shell: the parser of QML documents and of their JavaScript, the
//! runtime that instantiates the project's own `qml/` tree (objects, bindings, signals, models,
//! states) the way Qt 6 does, and the services behind its types (D-Bus, PipeWire, MPRIS,
//! network, notifications, ...). The shipped shell runs the tree's JavaScript translated to
//! Rust at build time (`crates/rigen`, run through `jsrt`); the tree-walking interpreter (the
//! `interp` feature, off in the shipped shell) is the reference that translation is checked
//! against. Drawing is `crates/shell`.

// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

mod animated;
pub mod ast;
pub mod bluez;
mod bluez_agent;
pub mod brightness;
pub mod builtins;
pub mod bytelru;
pub mod cava;
pub mod child;
pub mod childguard;
pub mod classname;
pub mod color;
pub mod colorscheme;
mod component;
pub mod context2d;
mod font_inherit;
mod palette;
pub mod props;
pub mod proxy;
mod qt_defaults;
mod qt_enums;
pub mod qurl;
mod qurlhost;
mod script_values;
pub use ::bus as dbus;
mod dbusvar;
pub mod debug;
pub mod desktop;
pub mod desktop_container;
mod detached;
pub mod easing;
pub mod easyeffects;
pub mod edit_mode;
pub mod emojis;
pub mod eval;
pub mod files;
pub mod flick;
pub mod fprint;
mod fswatch;
pub mod generator;
pub mod headless;
pub mod http;
pub mod hypr;
pub mod hyprconf;
pub mod hyprctl;
pub mod icons;
pub mod imgjobs;
pub mod impulse;
mod ini;
pub mod inputmethod;
#[cfg(feature = "interp")]
pub mod ir;
pub mod jobs;
pub mod jobs_ai;
pub mod jobs_files;
pub mod jobs_identify;
pub mod jobs_lists;
pub mod jobs_updates;
pub mod jobs_wall_gcloud_trans;
pub mod jobs_wallpick;
#[cfg(feature = "interp")]
pub mod js;
pub mod jslibs;
pub mod json;
pub mod jsrt;
pub mod key_order;
pub mod keyring;
pub mod latex;
pub mod layout;
pub mod layout_model;
pub mod libnotify;
pub mod locale;
mod locale_data;
pub mod locale_num;
pub mod magic;
pub mod markdown;
pub mod mask;
pub mod mpris;
pub mod musicrecognizer;
pub mod native;
pub mod network;
pub mod notifications;
pub mod notifier;
pub mod openurl;
pub mod osutil;
pub mod pam;
pub mod panel;
mod parser;
pub mod plan;
pub mod polkit;
pub mod ports;
pub mod position;
pub mod privfs;
pub mod probe;
pub mod processes;
pub mod procs;
pub mod promise;
pub mod pw;
pub mod qdatetime;
mod qqmljs;
pub mod qtobj;
mod qualified;
pub mod resolve;
pub mod runtime;
pub mod screenshot;
pub mod session;
pub mod shapes;
pub mod shell_config;
pub mod shellcontrol;
pub mod sni;
pub mod steppers;
pub mod svc;
pub mod symbol;
pub mod syntax;
pub mod sysbus;
pub mod system;
pub mod systemsound;
pub mod textpos;
pub mod transform;
pub mod typecheck;
pub mod typedarray;
pub mod types;
pub mod tz;
pub mod undo;
pub mod unwritten;
pub mod upower;
mod url;
pub mod utf16;
pub mod v4sort;
pub mod validator;
pub mod value;
pub mod valuetypes;
pub mod walk;
pub mod warning;
pub mod warp;
pub mod watchdog;
pub mod weather;
pub mod widget_settings;
pub mod widgets;
pub mod xdg;
pub mod xhr;
mod xhr_charset_data;
mod xhr_dom;
mod xhr_iso2022jp;
mod xhr_text;
mod zone_aliases;

#[cfg(test)]
mod userenv_sites;
#[cfg(test)]
use ::testenv;
#[cfg(test)]
pub(crate) use ::testenv::equiv_missing;

pub use ast::*;
pub use parser::{Error, parse};

/// Whether this build has the JS interpreter (feature `interp`), which runs every script
/// without a matching compiled entry.
pub const INTERP: bool = cfg!(feature = "interp");
