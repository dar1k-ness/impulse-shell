//! Ahead-of-time compiler of the original JS (QML bindings, handlers, functions, `.js` libraries) to Rust.
//!
//! [`walk`] assigns every script of a QML tree its registry key; [`qml_module`], [`js_module`]
//! and [`js_script`] lower the scripts of one source file (`qml::ir`, as the interpreter does)
//! into one Rust module. `crates/ricompiled-gen/build.rs` runs them over the tree and writes
//! the registry tables `crates/ricompiled` includes. The IR -> Rust emitter is [`emit_body`].

// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

mod expr;
mod scope;
mod stmt;
pub use qml::walk;

pub use scope::{HEADER, PRELUDE, emit_body, emit_function, take_literals};

use std::fmt::Write;
use std::path::{Path, PathBuf};
pub use walk::BodyKind;

/// What the emitter knows about the unit it compiles, besides its body and kind.
#[derive(Debug, Clone, Default)]
pub struct EmitCtx {
    /// Signal parameters known statically for a handler (the signal is declared on the
    /// handler's own object). The runtime also binds them as scope variables, so an empty list
    /// is always valid.
    pub params: Vec<String>,
    /// A `.js` file `Qt.include` runs in its caller's context: its top-level `var`s and
    /// functions are names of that context (`jsrt::declare_global`), not slots of its own.
    pub script: bool,
}

/// Coverage of a generation run.
#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub qml_files: usize,
    pub js_files: usize,
    pub bindings: usize,
    pub handlers: usize,
    pub functions: usize,
    /// `ir::Expr/Stmt::Unsupported` kinds found in lowered bodies.
    pub unsupported: Vec<(String, String)>,
    pub lines: usize,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "qml {} js {} bindings {} handlers {} functions {} unsupported {} generated lines {}",
            self.qml_files,
            self.js_files,
            self.bindings,
            self.handlers,
            self.functions,
            self.unsupported.len(),
            self.lines
        )
    }
}

/// Generated sources: `(file name, contents)`; `registry.rs` is the crate's entry table.
pub struct Generated {
    pub files: Vec<(String, String)>,
    pub stats: Stats,
}

/// `FnMeta` literal of an entry: the function's own for a `function` member (`[Stmt::Func]`),
/// else empty. `nslots` is informational (bodies make their own frame).
fn fn_meta(body: &[qml::ir::Stmt]) -> String {
    let (name, kind, nparams, strict) = match body {
        [qml::ir::Stmt::Func(f)] => (f.display_name(), f.kind, f.length(), f.strict),
        _ => ("", qml::ir::FnKind::Normal, 0, false),
    };
    format!(
        "FnMeta {{ name: {name:?}, kind: FnKind::{kind:?}, nparams: {nparams}, nslots: 0, strict: {strict} }}"
    )
}

fn unit_fn(out: &mut String, n: usize, code: &str) {
    writeln!(
        out,
        "fn k{n}(cx: &mut Cx, env: &Env, this: &Value, args: &[Value]) -> R<Value> {code}\n"
    )
    .unwrap();
}

/// Compile one QML document (key `file`, read from `path` in the QML tree at `root`) into a
/// module source. Types resolve as the runtime resolves them, so a site is lowered as the kind
/// the runtime runs it as ([`walk::binding_kind`]).
pub fn qml_module(
    file: &str,
    src: &str,
    path: &Path,
    root: &Path,
    stats: &mut Stats,
) -> Result<String, String> {
    let doc = qml::parse(src).map_err(|e| e.under(file))?;
    take_literals();
    let mut fns = String::new();
    let mut table = String::new();
    let mut n = 0;
    let mut err = None;
    let script_action = |t: &str| walk::is_script_action(root, path, &doc, t);
    walk::sites_typed(&doc, &script_action, &mut |s| {
        if err.is_some() {
            return;
        }
        let body = match qml::ir::lower(s.script) {
            Ok(b) => b,
            Err(e) => {
                err = Some(format!("{file}:{} {}: {e}", s.script.line, s.name));
                return;
            }
        };
        let key = format!("{file}|{}|{}", s.path, s.name);
        let mut u = Vec::new();
        qml::ir::unsupported(&body, &mut u);
        stats
            .unsupported
            .extend(u.into_iter().map(|k| (key.clone(), k)));
        match s.kind {
            BodyKind::Binding => stats.bindings += 1,
            BodyKind::Handler => stats.handlers += 1,
            _ => stats.functions += 1,
        }
        let ctx = EmitCtx {
            params: s.params.clone(),
            script: false,
        };
        unit_fn(&mut fns, n, &emit_body(&body, s.kind, &ctx));
        writeln!(
            table,
            "    Entry {{ file: {file:?}, path: {:?}, member: {:?}, kind: BodyKind::{}, f: k{n}, meta: {}, hash: {:#x} }},",
            s.path,
            s.name,
            s.kind.as_str(),
            fn_meta(&body),
            s.hash(),
        )
        .unwrap();
        n += 1;
    });
    if let Some(e) = err {
        return Err(e);
    }
    let mut layouts = String::new();
    walk::decls(&doc, &mut |path, _, names| {
        writeln!(layouts, "    ({path:?}, &{names:?}),").unwrap();
    });
    Ok(format!(
        "// @generated by rigen from {file}\n#![allow(unused, non_snake_case, clippy::all)]\n{HEADER}\nuse crate::abi::{{BodyKind, Entry}};\n\n{fns}{}\npub static ENTRIES: &[Entry] = &[\n{table}];\n\n/// `(objpath, property names in declaration order)`: slot `i` of a declaration is `names[i]`.\npub static LAYOUTS: &[(&str, &[&str])] = &[\n{layouts}];\n",
        take_literals()
    ))
}

/// Compile one `.js` library into a module source (`LIB`).
pub fn js_module(file: &str, src: &str, stats: &mut Stats) -> Result<String, String> {
    js_unit(file, src, stats, false)
}

/// Compile one `.js` file `Qt.include` runs in its caller's context into a module source
/// (`SCRIPT`).
pub fn js_script(file: &str, src: &str, stats: &mut Stats) -> Result<String, String> {
    js_unit(file, src, stats, true)
}

/// The files of `js` (a tree's `.js` files) that a file of `js` passes to `Qt.include` by a
/// literal URL, by index. A literal target outside the tree has no compiled code: the include
/// reports `EXCEPTION` at run time, so the generation reports it as unsupported.
pub fn include_targets(js: &[(String, PathBuf)], stats: &mut Stats) -> Result<Vec<usize>, String> {
    let canon: Vec<Option<PathBuf>> = js.iter().map(|(_, p)| p.canonicalize().ok()).collect();
    let mut out = std::collections::BTreeSet::new();
    for (key, p) in js {
        let src = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let body = qml::ir::lower_program(&src).map_err(|e| format!("{key}: {e}"))?;
        for url in qml::ir::include_targets(&body) {
            let target = qml::qtobj::resolve_include(&p.canonicalize().unwrap_or(p.clone()), &url)
                .and_then(|t| t.canonicalize().ok());
            match canon.iter().position(|c| c.is_some() && *c == target) {
                Some(i) => {
                    out.insert(i);
                }
                None => stats
                    .unsupported
                    .push((key.clone(), format!("Qt.include({url:?}) outside the tree"))),
            }
        }
    }
    Ok(out.into_iter().collect())
}

fn js_unit(file: &str, src: &str, stats: &mut Stats, script: bool) -> Result<String, String> {
    let body = qml::ir::lower_program(src).map_err(|e| format!("{file}: {e}"))?;
    take_literals();
    let mut u = Vec::new();
    qml::ir::unsupported(&body, &mut u);
    stats
        .unsupported
        .extend(u.into_iter().map(|k| (file.to_string(), k)));
    let ctx = EmitCtx {
        params: Vec::new(),
        script,
    };
    let mut out = format!(
        "// @generated by rigen from {file}\n#![allow(unused, non_snake_case, clippy::all)]\n{HEADER}\nuse crate::abi::{{BodyKind, Entry}};\n\n"
    );
    unit_fn(&mut out, 0, &emit_body(&body, BodyKind::Library, &ctx));
    out.push_str(&take_literals());
    writeln!(
        out,
        "pub static {}: Entry = Entry {{ file: {file:?}, path: \"\", member: \"\", kind: BodyKind::Library, f: k0, meta: {}, hash: {:#x} }};",
        if script { "SCRIPT" } else { "LIB" },
        fn_meta(&[]),
        walk::src_hash(src),
    )
    .unwrap();
    Ok(out)
}

/// Write `g` into `dir`, leaving files whose contents did not change untouched.
pub fn write(g: &Generated, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, s) in &g.files {
        let p = dir.join(name);
        if std::fs::read_to_string(&p).ok().as_deref() != Some(s) {
            std::fs::write(&p, s)?;
        }
    }
    Ok(())
}
