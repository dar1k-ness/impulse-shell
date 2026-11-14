use std::fmt::Write;
use std::path::{Path, PathBuf};

include!("src/host_cases.rs");

use rigen::{BodyKind, EmitCtx, HEADER, PRELUDE, emit_body, take_literals};

fn entry(name: &str, block: &str) -> String {
    format!(
        "pub fn {name}(cx: &mut Cx, env: &Env, this: &Value, args: &[Value]) -> R<Value> {block}\n"
    )
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, ext, out);
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

fn module(o: &mut String, name: &str, code: &str) {
    let lits = take_literals();
    let _ = writeln!(
        o,
        "#[allow(dead_code, non_snake_case, unused)]\npub mod {name} {{\n{HEADER}\n{code}\n{lits}}}"
    );
}

fn main() {
    let od = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(od.join("rt.rs"), PRELUDE).unwrap();
    println!("cargo:rerun-if-env-changed=RI_DIR");
    println!("cargo:rerun-if-changed=cases");
    println!("cargo:rerun-if-changed=src/host_cases.rs");
    println!("cargo:rerun-if-changed=../src");
    let mut out = String::new();
    let mut cases =
        String::from("pub static CASES: &[(&str, &str, &str, qml::jsrt::CompiledFn)] = &[\n");
    let mut files = Vec::new();
    walk(Path::new("cases"), "js", &mut files);
    files.sort();
    // `cases/included/`: the files cases pass to `Qt.include`, compiled as such (`INCLUDES`).
    let in_dir = |f: &Path, dir: &str| f.parent().and_then(Path::file_name) == Some(dir.as_ref());
    let (included, files): (Vec<_>, Vec<_>) =
        files.into_iter().partition(|f| in_dir(f, "included"));
    // `cases/qml/`: the files the QML cases import, compiled as libraries (`QML_LIBS`).
    let files: Vec<_> = files.into_iter().filter(|f| !in_dir(f, "qml")).collect();
    let mut includes = String::from("pub static INCLUDES: &[qml::jsrt::Entry] = &[\n");
    for (i, f) in included.iter().enumerate() {
        let src = std::fs::read_to_string(f).unwrap();
        let key = format!("inc_{i}");
        let body = qml::ir::lower_program(&src).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        let ctx = EmitCtx {
            script: true,
            ..EmitCtx::default()
        };
        module(
            &mut out,
            &key,
            &entry("entry", &emit_body(&body, BodyKind::Library, &ctx)),
        );
        let _ = writeln!(
            includes,
            "qml::jsrt::Entry {{ file: {:?}, path: \"\", member: \"\", kind: qml::jsrt::BodyKind::Library, f: {key}::entry, meta: qml::jsrt::FnMeta {{ name: \"\", kind: qml::jsrt::FnKind::Normal, nparams: 0, nslots: 0, strict: false }}, hash: {:#x} }},",
            std::fs::canonicalize(f).unwrap(),
            qml::walk::src_hash(&src),
        );
    }
    includes.push_str("];\n");
    out.push_str(&includes);
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap();
        let name = f.file_stem().unwrap().to_string_lossy().replace('-', "_");
        let key = format!("case_{name}");
        let body = qml::ir::lower_program(&src).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        module(
            &mut out,
            &key,
            &entry(
                "entry",
                &emit_body(&body, BodyKind::Library, &EmitCtx::default()),
            ),
        );
        let _ = writeln!(
            cases,
            "({name:?}, include_str!({path:?}), {path:?}, {key}::entry),",
            path = std::fs::canonicalize(f).unwrap()
        );
    }
    cases.push_str("];\n");
    out.push_str(&cases);
    qml_cases(&od, &mut out);
    // Every original script: compile check only.
    let (mut nqml, mut njs, mut nfail) = (0, 0, 0);
    if let Some(dir) = std::env::var_os("RI_DIR") {
        let dir = PathBuf::from(dir);
        let mut js = Vec::new();
        walk(&dir, "js", &mut js);
        js.sort();
        for (i, f) in js.iter().enumerate() {
            let src = std::fs::read_to_string(f).unwrap();
            let body = match qml::ir::lower_program(&src) {
                Ok(body) => body,
                Err(e) => {
                    println!("cargo:warning={}: {e}", f.display());
                    nfail += 1;
                    continue;
                }
            };
            let key = format!("lib_{i}");
            module(
                &mut out,
                &key,
                &entry(
                    "entry",
                    &emit_body(&body, BodyKind::Library, &EmitCtx::default()),
                ),
            );
            njs += 1;
        }
        let mut qml_files = Vec::new();
        walk(&dir, "qml", &mut qml_files);
        qml_files.sort();
        for (i, f) in qml_files.iter().enumerate() {
            let src = std::fs::read_to_string(f).unwrap();
            let doc = match qml::parse(&src) {
                Ok(doc) => doc,
                Err(e) => {
                    println!("cargo:warning={}: {e:?}", f.display());
                    nfail += 1;
                    continue;
                }
            };
            let mut code = String::new();
            let mut n = 0;
            doc.root.walk(&mut |o| {
                let mut ss = Vec::new();
                o.scripts(&mut ss);
                for s in ss {
                    n += 1;
                    let key = format!("q{i}_{n}");
                    let body = match qml::ir::lower(s) {
                        Ok(body) => body,
                        Err(e) => {
                            println!("cargo:warning={}:{}: {e}", f.display(), s.line);
                            nfail += 1;
                            continue;
                        }
                    };
                    let kind = if s.kind == qml::ScriptKind::Function {
                        BodyKind::Function
                    } else {
                        BodyKind::Binding
                    };
                    code.push_str(&entry(&key, &emit_body(&body, kind, &EmitCtx::default())));
                    nqml += 1;
                }
            });
            module(&mut out, &format!("qml_{i}"), &code);
        }
    }
    host_cases(&mut out);
    let _ = writeln!(
        out,
        "pub const RI_COUNTS: (usize, usize, usize) = ({nqml}, {njs}, {nfail});"
    );
    std::fs::write(od.join("gen.rs"), out).unwrap();
}

/// `src/host_cases.rs` compiled as QML-scope bodies (`HOST_FUNCS_C`, `HOST_CASES_C`), for `host.rs`.
fn host_cases(out: &mut String) {
    let script = |src: &str, kind| qml::Script::new(src, 0, kind);
    let ctx = |params: &[&str]| EmitCtx {
        params: params.iter().map(|p| p.to_string()).collect(),
        script: false,
    };
    let mut funcs = String::from(
        "pub static HOST_FUNCS_C: &[(qml::jsrt::CompiledFn, &qml::jsrt::FnMeta)] = &[\n",
    );
    for (i, (_, src)) in HOST_FUNCS.iter().enumerate() {
        let key = format!("host_f{i}");
        let body = qml::ir::lower(&script(src, qml::ScriptKind::Function))
            .unwrap_or_else(|e| panic!("{src}: {e}"));
        let [qml::ir::Stmt::Func(f)] = &body[..] else {
            panic!("{src}: not a function")
        };
        let block = rigen::emit_function(f);
        let meta = format!(
            "pub static META: qml::jsrt::FnMeta = qml::jsrt::FnMeta {{ name: {:?}, kind: qml::jsrt::FnKind::{:?}, nparams: {}, nslots: 0, strict: false }};\n",
            f.name.as_deref().unwrap_or(""),
            f.kind,
            f.length()
        );
        module(out, &key, &format!("{}{meta}", entry("entry", &block)));
        let _ = writeln!(funcs, "({key}::entry, &{key}::META),");
    }
    funcs.push_str("];\n");
    let mut cases = String::from("pub static HOST_CASES_C: &[qml::jsrt::CompiledFn] = &[\n");
    for (i, (_, kind, _, src, params)) in HOST_CASES.iter().enumerate() {
        let key = format!("host_c{i}");
        let sk = if src.trim_start().starts_with('{') {
            qml::ScriptKind::Block
        } else {
            qml::ScriptKind::Expression
        };
        let body = qml::ir::lower(&script(src, sk)).unwrap_or_else(|e| panic!("{src}: {e}"));
        let bk = if *kind == "handler" {
            BodyKind::Handler
        } else {
            BodyKind::Binding
        };
        module(
            out,
            &key,
            &entry("entry", &emit_body(&body, bk, &ctx(params))),
        );
        let _ = writeln!(cases, "{key}::entry,");
    }
    cases.push_str("];\n");
    out.push_str(&funcs);
    out.push_str(&cases);
}

/// Documents of `crates/qml/tests` that run here too, each with and without its compiled code
/// (`QML_FIXTURES`): what their own tests hold for the interpreter holds for compiled code.
const QML_FIXTURES: &[&str] = &[
    "../../qml/tests/data/builtins_b1/fn.qml",
    "../../qml/tests/data/builtins_b1/n3.qml",
    "../../qml/tests/runtime_audit_r3/R06MethodValue.qml",
];

/// Compiles QML documents `files` as the QML tree is, into the table `table` of
/// (name, source, path, entries, layouts).
fn qml_table(od: &Path, out: &mut String, table: &str, files: &[PathBuf]) {
    let mut rows = format!(
        "pub static {table}: &[(&str, &str, &str, &[qml::jsrt::Entry], &[qml::jsrt::Layout])] = &[\n",
    );
    let prefix = table.to_lowercase();
    for (i, f) in files.iter().enumerate() {
        println!("cargo:rerun-if-changed={}", f.display());
        let src = std::fs::read_to_string(f).unwrap();
        let dir = f.parent().unwrap();
        let key = f.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
        let mut stats = rigen::Stats::default();
        let m = rigen::qml_module(&key, &src, f, dir, &mut stats)
            .unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        assert!(
            stats.unsupported.is_empty(),
            "{}: {:?}",
            f.display(),
            stats.unsupported
        );
        let module_path = od.join(format!("{prefix}_{i}.rs"));
        std::fs::write(&module_path, m).unwrap();
        let _ = writeln!(
            out,
            "#[path = {:?}]\nmod {prefix}_{i};",
            module_path.display().to_string()
        );
        let name = f.file_stem().unwrap().to_string_lossy();
        let _ = writeln!(
            rows,
            "({name:?}, include_str!({path:?}), {path:?}, {prefix}_{i}::ENTRIES, {prefix}_{i}::LAYOUTS),",
            path = f.display().to_string()
        );
    }
    rows.push_str("];\n");
    out.push_str(&rows);
}

/// `cases/qml/*.qml` compiled as the QML tree is (`QML_CASES`), with the `.js` files they
/// import (`QML_LIBS`): documents a runtime loads with and without their compiled code. The
/// `.qml` files of a directory below are documents the cases load, not cases.
fn qml_cases(od: &Path, out: &mut String) {
    let dir = std::fs::canonicalize("cases/qml").unwrap();
    let mut files = Vec::new();
    walk(&dir, "qml", &mut files);
    files.retain(|f| f.parent() == Some(dir.as_path()));
    files.sort();
    qml_table(od, out, "QML_CASES", &files);
    let fixtures: Vec<_> = QML_FIXTURES
        .iter()
        .map(|f| std::fs::canonicalize(f).unwrap_or_else(|e| panic!("{f}: {e}")))
        .collect();
    qml_table(od, out, "QML_FIXTURES", &fixtures);
    let mut files = Vec::new();
    walk(&dir, "js", &mut files);
    files.sort();
    let mut libs = String::from("pub static QML_LIBS: &[(&str, &qml::jsrt::Entry)] = &[\n");
    for (i, f) in files.iter().enumerate() {
        let src = std::fs::read_to_string(f).unwrap();
        let key = f.strip_prefix(&dir).unwrap().to_string_lossy().into_owned();
        let mut stats = rigen::Stats::default();
        let m = rigen::js_module(&key, &src, &mut stats)
            .unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        assert!(
            stats.unsupported.is_empty(),
            "{}: {:?}",
            f.display(),
            stats.unsupported
        );
        let module_path = od.join(format!("qml_lib_{i}.rs"));
        std::fs::write(&module_path, m).unwrap();
        let _ = writeln!(
            out,
            "#[path = {:?}]\nmod qml_lib_{i};",
            module_path.display().to_string()
        );
        let _ = writeln!(libs, "({:?}, &qml_lib_{i}::LIB),", f.display().to_string());
    }
    libs.push_str("];\n");
    out.push_str(&libs);
}
