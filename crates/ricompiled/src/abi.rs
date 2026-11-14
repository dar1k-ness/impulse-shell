//! Names generated code is written against: the `qml::jsrt` ABI (docs/aot.md).
//!
//! The runtime runs an [`Entry`] with `jsrt::run_entry` over a root [`Env`] (the QML scope,
//! no frame; bodies open frames for captured bindings).
pub use qml::eval::R;
pub use qml::jsrt::{self, BodyKind, CompiledFn, Cx, Entry, Env, FnMeta, Layout};
pub use qml::value::Value;
pub use std::rc::Rc;
