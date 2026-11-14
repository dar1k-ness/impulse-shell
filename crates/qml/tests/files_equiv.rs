//! Wallpapers.qml `selectProc`/`validateDirProc` and LatexRenderer's MicroTeX Process: the native
//! jobs leave the QML side in the state the original commands, run by real bash/test, leave it in. And
//! the thumbnail writer (`jobs_files::thumbnail_code`) against what the original's `magick` command got wrong.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use qml::value::Value;
use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// The side a test takes (`mode`) is process-wide: one at a time.
static ENV: Mutex<()> = Mutex::new(());

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

fn tmp(n: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("files-equiv-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The original's QML before its Processes became native types: the reference side.
fn original() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/original")
}

/// The `<Type> { … id: <id> … }` block of `rel` under `tree`, verbatim.
fn block(tree: &Path, rel: &str, id: &str) -> String {
    let src = std::fs::read_to_string(tree.join(rel)).unwrap();
    let at = src.find(&format!("id: {id}\n")).unwrap();
    let line = src[..src[..at].rfind(" {\n").unwrap()].rfind('\n').unwrap() + 1;
    let start = line + src[line..].len() - src[line..].trim_start().len();
    let mut depth = 0;
    for (i, c) in src[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return src[start..start + i + 1].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("unterminated {id}")
}

/// Runs `test` in a process of its own (`testenv::run_in_own_process`) with private XDG homes
/// (the original's Config writes config.json under XDG_CONFIG_HOME: keep it off the user's) and `vars`;
/// true in that process, where the test's body runs.
fn own_process(test: &str, vars: Vec<(&'static str, OsString)>) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let home = tempfile::tempdir().unwrap();
    let xdg = [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CACHE_HOME", "cache"),
    ]
    .map(|(k, d)| (k, OsString::from(home.path().join(d))));
    testenv::run_in_own_process(test, xdg.into_iter().chain(vars));
    false
}

/// Only in a test's [`own_process`].
fn runtime() -> Runtime {
    let mut rt = Runtime::new(Some(ri()));
    let services: Vec<String> = std::fs::read_dir(ri().join("services"))
        .unwrap()
        .filter_map(|e| Some(e.ok()?.path().file_stem()?.to_str()?.to_string()))
        .collect();
    rt.load_singletons_where(
        &[
            ri().join("modules/common"),
            ri().join("modules/common/functions"),
            ri().join("services"),
        ],
        &|n| n == "LatexRenderer" || !services.iter().any(|s| s == n),
    );
    rt.settle();
    LOOP.set(Some(Loop::new(&mut rt)));
    rt
}

/// Loads `body` as the original's `rel` (inside a scratch tree, so the job lookup sees the original's file name).
fn load(rt: &mut Runtime, tree: &Path, rel: &str, body: &str) -> usize {
    let f = tree.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    let head = "import QtQuick\nimport Quickshell\nimport Rustshell.Io\nimport ri.modules.common\nimport ri.modules.common.functions\nimport ri.services\n";
    std::fs::write(&f, format!("{head}{body}")).unwrap();
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    root
}

fn call(rt: &mut Runtime, item: usize, f: &str, args: &[&str]) -> String {
    let a = args.iter().map(|s| Value::Str((*s).into())).collect();
    rt.call_method(item, f, a).unwrap().unwrap().to_js_string()
}

fn prop(rt: &mut Runtime, item: usize, name: &str) -> String {
    rt.world
        .borrow_mut()
        .read(item, name)
        .unwrap_or_default()
        .to_js_string()
}

thread_local!(static LOOP: RefCell<Option<Loop>> = const { RefCell::new(None) });

/// Runs the loop of the latest [`runtime`] until `done`, blocking for the events it waits on
/// (process output and exit), then until nothing more is ready.
fn pump(rt: &mut Runtime, done: &dyn Fn(&mut Runtime) -> bool) {
    LOOP.with_borrow_mut(|lp| {
        let lp = lp.as_mut().expect("a runtime()");
        lp.wait_until(rt, done);
        lp.drain(rt);
    });
}

fn sh(script: &str, args: &[&str]) -> (i32, String) {
    let o = std::process::Command::new("bash")
        .arg("-c")
        .arg(script)
        .arg("bash")
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

fn path_cases(d: &Path) -> Vec<String> {
    std::fs::create_dir_all(d.join("dir with space")).unwrap();
    std::fs::write(d.join("f.png"), b"x").unwrap();
    std::os::unix::fs::symlink(d.join("dir with space"), d.join("ldir")).unwrap();
    std::os::unix::fs::symlink(d.join("f.png"), d.join("lfile")).unwrap();
    std::os::unix::fs::symlink(d.join("nowhere"), d.join("dangling")).unwrap();
    let mut v: Vec<String> = [
        "dir with space",
        "f.png",
        "ldir",
        "lfile",
        "dangling",
        "missing",
    ]
    .iter()
    .map(|n| d.join(n).to_string_lossy().into_owned())
    .collect();
    v.push(format!("file://{}", d.join("f.png").display()));
    v.push(format!("file://{}/", d.join("dir with space").display()));
    v.push(format!("{}//", d.join("ldir").display()));
    v.extend(["", "/", "///"].map(String::from));
    v
}

#[test]
fn wallpaper_path_checks_match_test_and_bash() {
    let d = tmp("wp-direct");
    for p in path_cases(&d) {
        let t = p.strip_prefix("file://").unwrap_or(&p);
        assert_eq!(
            i32::from(qml::jobs_files::validate_dir_out(t) != "dir\n"),
            sh("test -d \"$1\"", &[t]).0,
            "{p}"
        );
        let script = format!(
            "if [ -d \"{t}\" ]; then echo dir; elif [ -f \"{t}\" ]; then echo file; else echo invalid; fi"
        );
        assert_eq!(
            qml::jobs_files::validate_dir_out(t),
            sh(&script, &[]).1,
            "{p}"
        );
    }
}

#[test]
fn wallpaper_processes_match_the_original() {
    if !own_process(
        concat!(module_path!(), "::wallpaper_processes_match_the_original"),
        vec![],
    ) {
        return;
    }
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let rel = "services/Wallpapers.qml";
    let body = |tree: &Path| {
        format!(
            "Item {{
    id: root
    property string directory: \"\"
    property var log: []
    property int done: 0
    function setDirectory(p) {{ log = log.concat([\"dir \" + p]) }}
    function apply(p, d) {{ log = log.concat([\"apply \" + p]) }}
    function run(p) {{ selectProc.select(p, false) }}
    function validate(p) {{ validateDirProc.setDirectoryIfValid(p) }}
    function state() {{ return JSON.stringify([log, directory.toString()]) }}
    Connections {{ target: selectProc; function onExited(c, s) {{ root.done++ }} }}
    Connections {{ target: validateDirProc; function onExited(c, s) {{ root.done++ }} }}
    {}
    {}
}}
",
            block(tree, rel, "selectProc"),
            block(tree, rel, "validateDirProc")
        )
    };
    let d = tmp("wp");
    let cases = path_cases(&d.join("files"));
    let mut got = Vec::new();
    for native in [false, true] {
        let mut rt = runtime();
        let tree = if native { ri() } else { original() };
        let root = load(
            &mut rt,
            &d.join(if native { "n" } else { "o" }),
            rel,
            &body(&tree),
        );
        let mut out = Vec::new();
        for (i, p) in cases.iter().enumerate() {
            for f in ["run", "validate"] {
                rt.call_method(root, f, vec![Value::Str(p.as_str().into())]);
                let want = (2 * i + usize::from(f == "validate") + 1) as f64;
                pump(
                    &mut rt,
                    &|rt| matches!(rt.world.borrow_mut().read(root, "done"), Some(Value::Num(n)) if n >= want),
                );
                let (log, directory): (Vec<String>, String) =
                    serde_json::from_str(&call(&mut rt, root, "state", &[])).unwrap();
                out.push((format!("{f} {p}"), log, directory));
            }
        }
        got.push(out);
    }
    // Intended (docs/original-differences.md): the folder changes only once the path is known to be
    // a folder or a file. The original set it to the path as asked before the answer, so a path that is
    // neither stayed as the folder until the next valid one. The steps where that shows, with
    // the folder of the original and the one here; every other step is the same on both sides.
    let files = d.join("files");
    let at = |n: &str| format!("file://{}", files.join(n).display());
    let path = |n: &str| files.join(n).display().to_string();
    let kept = format!("file://{}", files.display());
    let mut differ = std::collections::BTreeMap::from([
        (
            format!("validate {}", path("dangling")),
            (at("dangling"), kept.clone()),
        ),
        (
            format!("run {}", path("missing")),
            (at("dangling"), kept.clone()),
        ),
        (
            format!("validate {}", path("missing")),
            (at("missing"), kept.clone()),
        ),
        (
            format!("run {}", at("f.png")),
            (at("missing"), kept.clone()),
        ),
    ]);
    assert_eq!(got[0].len(), got[1].len());
    for ((step, o_log, o_dir), (n_step, n_log, n_dir)) in got[0].iter().zip(&got[1]) {
        assert_eq!(n_step, step);
        assert_eq!(n_log, o_log, "{step}: processes");
        match differ.remove(step) {
            Some((ri_dir, dir)) => {
                assert_eq!(o_dir, &ri_dir, "{step}: the original's folder");
                assert_eq!(n_dir, &dir, "{step}: folder");
            }
            None => assert_eq!(n_dir, o_dir, "{step}: folder"),
        }
    }
    assert!(differ.is_empty(), "steps that never ran: {differ:?}");
}

/// The original runs `magick`; with only ImageMagick 6 installed, `convert` stands in for it.
fn magick_path(d: &Path) -> Option<String> {
    let has = |c: &str| {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {c}"))
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let path = std::env::var("PATH").unwrap_or_default();
    if has("magick") {
        return Some(path);
    }
    if !has("convert") {
        return None;
    }
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let m = bin.join("magick");
    testenv::install_script(&m, "#!/bin/sh\nexec convert \"$@\"\n");
    Some(format!("{}:{path}", bin.display()))
}

fn png_size(p: &Path) -> Option<(u32, u32)> {
    let b = std::fs::read(p).ok()?;
    let n = |i: usize| u32::from_be_bytes(b[i..i + 4].try_into().unwrap());
    (b.len() > 24 && &b[1..4] == b"PNG").then(|| (n(16), n(20)))
}

/// Runs `test` in its [`own_process`], whose PATH finds `magick` (ImageMagick 6's `convert`
/// behind a shim in `tmp(tag)` where there is no `magick`); there, the directory holding the
/// test images, and ENV.
fn thumb_env(test: &str, tag: &str) -> Option<(PathBuf, MutexGuard<'static, ()>)> {
    if !testenv::in_own_process(test) {
        let d = tmp(tag);
        let Some(path) = magick_path(&d) else {
            testenv::unavailable("ImageMagick (`magick`) is not installed");
            return None;
        };
        own_process(
            test,
            vec![("PATH", path.into()), ("FILES_EQUIV_THUMB_DIR", d.into())],
        );
        return None;
    }
    let g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let d = PathBuf::from(std::env::var_os("FILES_EQUIV_THUMB_DIR").unwrap());
    let src = d.join("src.png");
    let ok = std::process::Command::new("magick")
        .args(["-size", "500x300", "gradient:red-blue"])
        .arg(&src)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "magick cannot write {}", src.display());
    std::fs::write(d.join("bad.png"), b"not an image").unwrap();
    Some((d, g))
}

/// The original's `[ -f T ] && exit 0 || { magick S -resize NxN T && exit 1; }` broke on `'` in a path
/// and on a missing size directory, and never noticed a changed source. The writer every
/// thumbnail of the shell goes through (`DirectoryThumbnails`) returns 0 for a current
/// thumbnail, 1 for one it wrote and 2 for one it could not write.
#[test]
fn thumbnail_writer_fixes_the_original() {
    use std::os::unix::fs::PermissionsExt;
    let test = concat!(module_path!(), "::thumbnail_writer_fixes_the_original");
    let Some((d, _g)) = thumb_env(test, "thumb-fix") else {
        return;
    };
    let quoted = d.join("it's a pic.png");
    std::fs::copy(d.join("src.png"), &quoted).unwrap();
    let q = quoted.to_string_lossy().into_owned();
    let bad = d.join("bad.png").to_string_lossy().into_owned();
    let out = d.join("cache");
    let o = |n: &str| out.join(n).to_string_lossy().into_owned();
    let thumb = |s: &str, n: &str, t: &str| qml::jobs_files::thumbnail_code(s, n, t).0;
    let mode_of = |p: &str| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

    // `'` in both paths, and a size directory that does not exist yet.
    let t = o("new/large/it's.png");
    assert_eq!(thumb(&format!("file://{q}"), "256", &t), 1);
    assert_eq!(png_size(Path::new(&t)), Some((256, 154)));
    assert_eq!(mode_of(&o("new/large")), 0o700);
    assert_eq!(mode_of(&t), 0o600);
    let mtime = |p: &Path| {
        std::fs::metadata(p)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string()
    };
    let text = |k: &str| imgtools::magick::png_text(Path::new(&t), k);
    assert_eq!(text("Thumb::MTime"), Some(mtime(&quoted)));
    let uri = format!("file://{}", q.replace(' ', "%20"));
    assert_eq!(text("Thumb::URI"), Some(uri));
    assert_eq!(thumb(&q, "256", &t), 0);

    // A changed source is noticed and its thumbnail replaced.
    let f = std::fs::File::options().write(true).open(&quoted).unwrap();
    f.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_000_000))
        .unwrap();
    drop(f);
    assert_eq!(thumb(&q, "128", &t), 1);
    assert_eq!(png_size(Path::new(&t)), Some((128, 77)));
    assert_eq!(text("Thumb::MTime"), Some("1000000".into()));
    assert_eq!(thumb(&q, "128", &t), 0);

    // Failures return 2 and leave nothing behind.
    assert_eq!(thumb(&bad, "256", &o("bad.png")), 2);
    assert_eq!(thumb(&q, "bogus", &o("bogus.png")), 2);
    let missing = d.join("missing.png").to_string_lossy().into_owned();
    assert_eq!(thumb(&missing, "256", &o("m.png")), 2);
    let mut left: Vec<String> = Vec::new();
    for dir in [out.clone(), out.join("new/large")] {
        for e in std::fs::read_dir(dir).unwrap() {
            left.push(e.unwrap().file_name().to_string_lossy().into_owned());
        }
    }
    left.sort();
    assert_eq!(left, ["it's.png", "new"]);
}

const LATEX_BODY: &str = "Item {
    id: root
    property var finished: []
    Connections { target: LatexRenderer; function onRenderFinished(h, p) { root.finished = root.finished.concat([h + \" \" + p]) } }
    function setup(dir, name, out) { LatexRenderer.microtexBinaryDir = dir; LatexRenderer.microtexBinaryName = name; LatexRenderer.latexOutputPath = out }
    function render(e) { try { return JSON.stringify(LatexRenderer.requestRender(e)) } catch (x) { return \"throw\" } }
    function count() { return root.finished.length }
    function state() { return JSON.stringify([LatexRenderer.processedHashes, LatexRenderer.processedExpressions, LatexRenderer.renderedImagePaths]) }
}
";

/// A MicroTeX stand-in: logs its cwd, argv[0] and arguments (one write, so renders running
/// at once don't interleave), and writes the `-input` text as the SVG (or fails, with no SVG,
/// for `-input=fail`). `-input=slow` first blocks reading the FIFO `bin/gate` until the test
/// writes a line to it.
fn microtex(bin: &Path, log: &Path) {
    std::fs::create_dir_all(bin).unwrap();
    let gate = bin.join("gate");
    assert!(
        Command::new("mkfifo")
            .arg(&gate)
            .status()
            .unwrap()
            .success()
    );
    let exe = bin.join("LaTeX");
    testenv::install_script(
        &exe,
        &format!(
            "#!/bin/sh\ns=; for a in \"$@\"; do s=\"$s[$a]\"; done\n\
             printf '%s\\n%s\\n%s\\n' \"$(pwd)\" \"$0\" \"$s\" >> '{}'\n\
             for a in \"$@\"; do case $a in -input=*) i=${{a#-input=}};; -output=*) o=${{a#-output=}};; esac; done\n\
             [ \"$i\" = fail ] && exit 1\n[ \"$i\" = slow ] && read _ < '{}'\nprintf '%s' \"$i\" > \"$o\"; echo out; echo err >&2\n",
            log.display(),
            gate.display()
        ),
    );
}

fn finished_at_least(rt: &mut Runtime, root: usize, n: usize) {
    pump(rt, &|rt| {
        matches!(rt.world.borrow_mut().read(root, "finished"),
            Some(Value::Obj(o)) if o.borrow().array.as_ref().is_some_and(|a| a.len() >= n))
    });
}

/// The MicroTeX runs of the tree's LatexRenderer whose exit it has not handled yet: each is a
/// Process under it until then.
fn microtex_procs(rt: &mut Runtime) -> usize {
    let mut w = rt.world.borrow_mut();
    let lr = w.singletons["LatexRenderer"];
    w.of_type("Process")
        .into_iter()
        .filter(|&p| w.items[p].parent == Some(lr))
        .count()
}

/// The `renderFinished` signals so far, in order.
fn latex_finished(rt: &mut Runtime, root: usize) -> Vec<String> {
    let fin = prop(rt, root, "finished");
    fin.split(',')
        .filter(|f| !f.is_empty())
        .map(String::from)
        .collect()
}

fn latex_state(rt: &mut Runtime, root: usize) -> serde_json::Value {
    serde_json::from_str(&call(rt, root, "state", &[])).unwrap()
}

/// Intended (docs/original-differences.md): a formula MicroTeX failed on is rendered again by the
/// next request, and an empty formula is not rendered. The original kept the failed one as processed and
/// reported an image that was never made, and ran MicroTeX on the empty one. Each step with
/// what the original answers and what the tree answers.
#[test]
fn latex_failed_and_empty_renders_differ_from_the_original() {
    let test = concat!(
        module_path!(),
        "::latex_failed_and_empty_renders_differ_from_the_original"
    );
    if !own_process(test, vec![]) {
        return;
    }
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let d = tmp("latex-failed");
    let hash = |e: &str| qml::builtins::md5_hex(e.as_bytes());
    let (fail, empty) = (hash("fail"), hash(""));
    for native in [false, true] {
        let side = d.join(if native { "n" } else { "o" });
        let bin = side.join("microtex");
        let log = side.join("log");
        microtex(&bin, &log);
        let out = side.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let mut rt = runtime();
        let root = latex_root(&mut rt, native, &side, LATEX_BODY);
        let _ = call(
            &mut rt,
            root,
            "setup",
            &[&bin.to_string_lossy(), "LaTeX", &out.to_string_lossy()],
        );
        // The stand-in writes three lines for each time it is started.
        let runs = || {
            std::fs::read_to_string(&log)
                .unwrap_or_default()
                .lines()
                .count()
                / 3
        };
        let image = |h: &str| format!("{}/{h}.svg", out.display());
        let reported = |h: &str| format!("{h} {}", image(h));
        let nothing = serde_json::json!([[], {}, {}]);

        assert_eq!(
            call(&mut rt, root, "render", &["fail"]),
            format!("[\"{fail}\",true]")
        );
        if native {
            pump(&mut rt, &|rt| microtex_procs(rt) == 0);
            assert_eq!(latex_state(&mut rt, root), nothing);
            assert_eq!(latex_finished(&mut rt, root), [""; 0]);
        } else {
            finished_at_least(&mut rt, root, 1);
            assert_eq!(
                latex_state(&mut rt, root),
                serde_json::json!([[fail], {&fail: "fail"}, {&fail: image(&fail)}])
            );
            assert_eq!(latex_finished(&mut rt, root), [reported(&fail)]);
        }
        assert_eq!(runs(), 1);

        // The same formula again.
        let again = call(&mut rt, root, "render", &["fail"]);
        if native {
            assert_eq!(again, format!("[\"{fail}\",true]"));
            pump(&mut rt, &|rt| microtex_procs(rt) == 0);
            assert_eq!(runs(), 2);
            assert_eq!(latex_state(&mut rt, root), nothing);
            assert_eq!(latex_finished(&mut rt, root), [""; 0]);
        } else {
            assert_eq!(again, format!("[\"{fail}\",false]"));
            pump(&mut rt, &|_| true);
            assert_eq!(runs(), 1);
            assert_eq!(
                latex_finished(&mut rt, root),
                [reported(&fail), reported(&fail)]
            );
        }

        // The empty formula; the stand-in, unlike MicroTeX, makes an image of it.
        let started = runs();
        let none = call(&mut rt, root, "render", &[""]);
        if native {
            assert_eq!(none, format!("[\"{empty}\",false]"));
            assert_eq!(microtex_procs(&mut rt), 0);
            pump(&mut rt, &|_| true);
            assert_eq!(runs(), started);
            assert_eq!(latex_state(&mut rt, root), nothing);
            assert_eq!(latex_finished(&mut rt, root), [""; 0]);
            assert_eq!(svgs(&out), []);
        } else {
            assert_eq!(none, format!("[\"{empty}\",true]"));
            finished_at_least(&mut rt, root, 3);
            assert_eq!(runs(), started + 1);
            assert_eq!(
                latex_state(&mut rt, root),
                serde_json::json!([
                    [fail, empty],
                    {&fail: "fail", &empty: ""},
                    {&fail: image(&fail), &empty: image(&empty)}
                ])
            );
            assert_eq!(
                latex_finished(&mut rt, root),
                [reported(&fail), reported(&fail), reported(&empty)]
            );
            assert_eq!(svgs(&out), [(format!("{empty}.svg"), String::new())]);
        }
    }
}

#[test]
fn latex_render_matches_the_original() {
    if !own_process(
        concat!(module_path!(), "::latex_render_matches_the_original"),
        vec![],
    ) {
        return;
    }
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let d = tmp("latex");
    let exprs = ["\\frac{a}{b}", "$HOME; `x` \\\\ \\n end", " "];
    let mut got = Vec::new();
    for native in [false, true] {
        let side = d.join(if native { "n" } else { "o" });
        let bin = side.join("Micro TeX");
        let log = side.join("log");
        microtex(&bin, &log);
        let out = side.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let mut rt = runtime();
        let root = latex_root(&mut rt, native, &side, LATEX_BODY);
        // The original's bash runs `cd DIR` unquoted, so its directory needs no space.
        let link = side.join("microtex");
        std::os::unix::fs::symlink(&bin, &link).unwrap();
        let _ = call(
            &mut rt,
            root,
            "setup",
            &[&link.to_string_lossy(), "LaTeX", &out.to_string_lossy()],
        );
        let mut res = Vec::new();
        for e in exprs {
            res.push(format!("{e:?} -> {}", call(&mut rt, root, "render", &[e])));
        }
        finished_at_least(&mut rt, root, exprs.len());
        let side_s = side.to_string_lossy().into_owned();
        let mut fin: Vec<String> = prop(&mut rt, root, "finished")
            .replace(&side_s, "SIDE")
            .split(',')
            .map(String::from)
            .collect();
        fin.sort();
        res.push(format!("{fin:?}"));
        let mut lines: Vec<String> = std::fs::read_to_string(&log)
            .unwrap()
            .replace(&side_s, "SIDE")
            .lines()
            .map(String::from)
            .collect();
        lines.sort();
        res.push(format!("{lines:?}"));
        let mut st: Vec<char> = call(&mut rt, root, "state", &[])
            .replace(&side_s, "SIDE")
            .chars()
            .collect();
        st.sort();
        res.push(st.into_iter().collect());
        got.push(res);
    }
    assert_eq!(got[0], got[1]);
}

/// Where the original's generated QML or bash quoting breaks, or it reports a render that never
/// happened, the native renderer does the intended thing.
#[test]
fn latex_render_fixes_the_original() {
    if !own_process(
        concat!(module_path!(), "::latex_render_fixes_the_original"),
        vec![],
    ) {
        return;
    }
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let d = tmp("latex-fix");
    let bin = d.join("Micro TeX");
    let log = d.join("log");
    microtex(&bin, &log);
    let out = d.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut rt = runtime();
    let root = load(&mut rt, &d.join("tree"), "shell.qml", LATEX_BODY);
    let _ = call(
        &mut rt,
        root,
        "setup",
        &[&bin.to_string_lossy(), "LaTeX", &out.to_string_lossy()],
    );
    let hash = |e: &str| qml::builtins::md5_hex(e.as_bytes());
    let fresh = |e: &str| format!("[\"{}\",true]", hash(e));
    let exprs = [
        "it's",
        "a'b'c d'e'f",
        "x\"y",
        "two\nlines",
        "back\\slash $HOME",
        "slow",
    ];
    for e in exprs {
        assert_eq!(call(&mut rt, root, "render", &[e]), fresh(e), "{e:?}");
    }
    // A repeat while the first is still rendering reports nothing yet.
    assert_eq!(
        call(&mut rt, root, "render", &["slow"]),
        format!("[\"{}\",false]", hash("slow"))
    );
    assert_eq!(call(&mut rt, root, "count", &[]), "0");
    let gate = bin.join("gate");
    std::thread::spawn(move || std::fs::write(gate, "\n").unwrap());
    finished_at_least(&mut rt, root, exprs.len());
    for e in exprs {
        let svg = out.join(format!("{}.svg", hash(e)));
        assert_eq!(std::fs::read_to_string(&svg).unwrap(), e, "{e:?}");
    }
    assert_eq!(call(&mut rt, root, "count", &[]), exprs.len().to_string());
    let log_s = std::fs::read_to_string(&log).unwrap();
    assert!(
        log_s.contains(&format!("{}\n./LaTeX\n", bin.display())),
        "{log_s}"
    );
    // Once rendered, a repeat is reported at once.
    let _ = call(&mut rt, root, "render", &["it's"]);
    assert_eq!(
        call(&mut rt, root, "count", &[]),
        (exprs.len() + 1).to_string()
    );

    // Failed or impossible renders report nothing and leave no image.
    assert_eq!(call(&mut rt, root, "render", &["fail"]), fresh("fail"));
    let _ = call(
        &mut rt,
        root,
        "setup",
        &[&bin.to_string_lossy(), "nope", &out.to_string_lossy()],
    );
    assert_eq!(call(&mut rt, root, "render", &["gone"]), fresh("gone"));
    let _ = call(
        &mut rt,
        root,
        "setup",
        &[&bin.to_string_lossy(), "LaTeX", &out.to_string_lossy()],
    );
    let _ = call(&mut rt, root, "render", &["after"]);
    finished_at_least(&mut rt, root, exprs.len() + 2);
    // Every MicroTeX run has exited and reported (or not) by the time its Process is gone.
    pump(&mut rt, &|rt| microtex_procs(rt) == 0);
    assert_eq!(
        call(&mut rt, root, "count", &[]),
        (exprs.len() + 2).to_string()
    );
    for e in ["fail", "gone"] {
        assert!(!out.join(format!("{}.svg", hash(e))).exists(), "{e}");
        let rendered = format!("\"{}\":\"{}", hash(e), out.display());
        assert!(
            !call(&mut rt, root, "state", &[]).contains(&rendered),
            "{e}"
        );
    }
    assert_eq!(microtex_procs(&mut rt), 0, "MicroTeX Processes left behind");
}

#[test]
fn latex_run_reports_missing_binaries() {
    let d = tmp("latex-run");
    let file = d.join("file");
    std::fs::write(&file, "").unwrap();
    let dir = d.join("bin");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("noexec"), "").unwrap();
    for (dir, name, want) in [
        (d.join("none"), "LaTeX", 127),
        (file, "LaTeX", 127),
        (dir.clone(), "LaTeX", 127),
        (dir, "noexec", 126),
    ] {
        let (code, out, err) =
            qml::latex::run(&dir.to_string_lossy(), name, &[], &Default::default());
        assert_eq!(
            (code, out.as_str()),
            (want, ""),
            "{}/{name}: {err}",
            dir.display()
        );
        assert!(
            err.starts_with(&format!("{}/{name}: ", dir.display())),
            "{err}"
        );
    }
}

/// LATEX_BODY with the look arguments, for the real MicroTeX.
const LATEX_LOOK_BODY: &str = "Item {
    id: root
    property var finished: []
    Connections { target: LatexRenderer; function onRenderFinished(h, p) { root.finished = root.finished.concat([h + \" \" + p]) } }
    function setup(dir, out) { LatexRenderer.microtexBinaryDir = dir; LatexRenderer.latexOutputPath = out }
    function render(e) { try { return JSON.stringify(LatexRenderer.requestRender(e)) } catch (x) { return \"throw\" } }
    function state() { return JSON.stringify([LatexRenderer.processedHashes, LatexRenderer.processedExpressions, LatexRenderer.renderedImagePaths]) }
    function look() { return Appearance.font.pixelSize.normal + \"|\" + LatexRenderer.renderPadding + \"|\" + Appearance.colors.colOnLayer1 }
}
";

/// Expressions the original's bash command passes through intact.
const LATEX_PLAIN: [&str; 4] = [
    "\\frac{a}{b}",
    "x^2 + y^2 = z^2 $HOME; `x` \\\\ \\n end",
    "\\begin{pmatrix} 1 & 2 \\\\ 3 & 4 \\end{pmatrix}",
    "\\frac{a}{b}",
];

/// Expressions the original breaks: its generated QML fails to parse on `"` and newlines (requestRender
/// throws), and an odd number of `'` leaves bash's quoting open (MicroTeX never runs) while an
/// even number is dropped. The native renderer passes them to MicroTeX as written.
const LATEX_BROKEN: [&str; 5] = [
    "it's",
    "f'(x) = g''(x)",
    "\\text{x\"y}",
    "two\nlines",
    "a'b'c d'e'f",
];

/// Renders `exprs` with the MicroTeX in `dir` into `out`: each call's result, the sorted
/// `renderFinished` signals, the singleton's state, and the look arguments.
fn latex_side(
    native: bool,
    side: &Path,
    dir: &str,
    out: &Path,
    exprs: &[&str],
    want: usize,
) -> (Vec<String>, Vec<String>, serde_json::Value, String) {
    let mut rt = runtime();
    let root = latex_root(&mut rt, native, side, LATEX_LOOK_BODY);
    let _ = call(&mut rt, root, "setup", &[dir, &out.to_string_lossy()]);
    let mut res = Vec::new();
    for e in exprs {
        res.push(format!("{e:?} -> {}", call(&mut rt, root, "render", &[e])));
    }
    pump(&mut rt, &|rt| {
        matches!(rt.world.borrow_mut().read(root, "finished"),
            Some(Value::Obj(o)) if o.borrow().array.as_ref().is_some_and(|a| a.len() >= want))
    });
    let mut fin: Vec<String> = prop(&mut rt, root, "finished")
        .split(',')
        .map(String::from)
        .collect();
    fin.retain(|f| !f.is_empty());
    fin.sort();
    let state = serde_json::from_str(&call(&mut rt, root, "state", &[])).unwrap();
    let look = call(&mut rt, root, "look", &[]);
    (res, fin, state, look)
}

/// `body` loaded on `side`: natively it uses the tree's LatexRenderer, else the original's singleton from
/// [`original`] as an object of the scratch tree.
fn latex_root(rt: &mut Runtime, native: bool, side: &Path, body: &str) -> usize {
    if native {
        return load(rt, &side.join("tree"), "shell.qml", body);
    }
    let ri_src = std::fs::read_to_string(original().join("services/LatexRenderer.qml"))
        .unwrap()
        .replace("pragma Singleton\n", "");
    std::fs::create_dir_all(side.join("tree")).unwrap();
    std::fs::write(side.join("tree/RustyLatexRenderer.qml"), ri_src).unwrap();
    let body = body.replace("LatexRenderer", "latexRenderer").replacen(
        "    id: root\n",
        "    id: root\n    RustyLatexRenderer { id: latexRenderer }\n",
        1,
    );
    load(rt, &side.join("tree"), "shell.qml", &body)
}

fn svgs(out: &Path) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(out)
        .unwrap()
        .flatten()
        .map(|e| {
            let svg = std::fs::read_to_string(e.path()).unwrap_or_default();
            (e.file_name().to_string_lossy().into_owned(), svg)
        })
        .collect();
    v.sort();
    v
}

/// The MicroTeX the original installs (`/opt/MicroTeX`, or `RI_MICROTEX_DIR`): the SVGs, signals and
/// state match the original's bash commands where those work, and where the original breaks the SVG is the one
/// MicroTeX renders for the expression as written.
#[test]
fn latex_render_matches_real_microtex() {
    let dir = std::env::var("RI_MICROTEX_DIR").unwrap_or_else(|_| "/opt/MicroTeX".into());
    if !Path::new(&dir).join("LaTeX").is_file() {
        testenv::unavailable(format!("MicroTeX ({dir}/LaTeX)"));
        return;
    }
    let test = concat!(module_path!(), "::latex_render_matches_real_microtex");
    if !own_process(test, vec![]) {
        return;
    }
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let d = tmp("latex-real");
    let out = d.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let n = LATEX_PLAIN.len();
    let (calls, fin, state, _) = latex_side(false, &d.join("o"), &dir, &out, &LATEX_PLAIN, n);
    let images: Vec<_> = svgs(&out)
        .into_iter()
        .filter(|(_, s)| s.contains("<svg"))
        .collect();
    assert!(
        images.len() >= 3,
        "MicroTeX rendered too little: {images:?}"
    );
    let rendered: std::collections::BTreeSet<String> = images
        .iter()
        .map(|(f, _)| f.trim_end_matches(".svg").to_string())
        .collect();
    std::fs::remove_dir_all(&out).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let (n_calls, n_fin, n_state, _) =
        latex_side(true, &d.join("n"), &dir, &out, &LATEX_PLAIN, rendered.len());
    assert_eq!(n_calls, calls);
    // Intended: renderFinished once per image MicroTeX made, never for a failed render or a
    // repeated request still rendering (the original reported both, showing a broken image).
    let is_rendered = |f: &String| rendered.iter().any(|h| f.starts_with(h.as_str()));
    let mut want_fin: Vec<String> = fin.into_iter().filter(is_rendered).collect();
    want_fin.dedup();
    assert_eq!(n_fin, want_fin);
    assert_eq!(n_state[0], state[0], "processedHashes");
    assert_eq!(n_state[1], state[1], "processedExpressions");
    let paths = |v: &serde_json::Value| -> std::collections::BTreeMap<String, String> {
        v.as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
            .collect()
    };
    let mut want_paths = paths(&state[2]);
    want_paths.retain(|h, _| rendered.contains(h));
    assert_eq!(paths(&n_state[2]), want_paths, "renderedImagePaths");
    assert_eq!(svgs(&out), images);

    let out = d.join("broken");
    std::fs::create_dir_all(&out).unwrap();
    let n = LATEX_BROKEN.len();
    let (_, _, _, look) = latex_side(true, &d.join("b"), &dir, &out, &LATEX_BROKEN, n);
    let v: Vec<&str> = look.split('|').collect();
    let direct = d.join("direct");
    std::fs::create_dir_all(&direct).unwrap();
    for e in LATEX_BROKEN {
        let image = format!(
            "{}/{}.svg",
            direct.display(),
            qml::builtins::md5_hex(e.as_bytes())
        );
        let st = std::process::Command::new("./LaTeX")
            .args(qml::latex::microtex_args(e, &image, v[0], v[1], v[2]))
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(st.status.success(), "{e:?}");
    }
    let (ours, direct) = (svgs(&out), svgs(&direct));
    assert_eq!(ours.len(), LATEX_BROKEN.len(), "{ours:?}");
    assert_eq!(ours, direct);
}

/// The same difference as `latex_failed_and_empty_renders_differ_from_the_original` with the MicroTeX the original
/// installs, which fails on `}` and on the empty formula: the original keeps both as processed with an
/// image that is not there; the tree keeps neither, renders `}` again and never starts
/// MicroTeX for the empty one.
#[test]
fn latex_real_microtex_failures_differ_from_the_original() {
    let dir = std::env::var("RI_MICROTEX_DIR").unwrap_or_else(|_| "/opt/MicroTeX".into());
    if !Path::new(&dir).join("LaTeX").is_file() {
        testenv::unavailable(format!("MicroTeX ({dir}/LaTeX)"));
        return;
    }
    let test = concat!(
        module_path!(),
        "::latex_real_microtex_failures_differ_from_the_original"
    );
    if !own_process(test, vec![]) {
        return;
    }
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let d = tmp("latex-real-failed");
    let hash = |e: &str| qml::builtins::md5_hex(e.as_bytes());
    let (brace, empty) = (hash("}"), hash(""));
    for native in [false, true] {
        let side = d.join(if native { "n" } else { "o" });
        let out = side.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let mut rt = runtime();
        let root = latex_root(&mut rt, native, &side, LATEX_LOOK_BODY);
        let _ = call(&mut rt, root, "setup", &[&dir, &out.to_string_lossy()]);
        let image = |h: &str| format!("{}/{h}.svg", out.display());
        let reported = |h: &str| format!("{h} {}", image(h));
        let nothing = serde_json::json!([[], {}, {}]);
        let mut calls = Vec::new();
        for e in ["}", "}", ""] {
            calls.push(call(&mut rt, root, "render", &[e]));
            if native {
                pump(&mut rt, &|rt| microtex_procs(rt) == 0);
                assert_eq!(latex_state(&mut rt, root), nothing, "{e:?}");
            } else {
                finished_at_least(&mut rt, root, calls.len());
            }
        }
        if native {
            assert_eq!(
                calls,
                [
                    format!("[\"{brace}\",true]"),
                    format!("[\"{brace}\",true]"),
                    format!("[\"{empty}\",false]")
                ]
            );
            assert_eq!(latex_finished(&mut rt, root), [""; 0]);
        } else {
            assert_eq!(
                calls,
                [
                    format!("[\"{brace}\",true]"),
                    format!("[\"{brace}\",false]"),
                    format!("[\"{empty}\",true]")
                ]
            );
            assert_eq!(
                latex_state(&mut rt, root),
                serde_json::json!([
                    [brace, empty],
                    {&brace: "}", &empty: ""},
                    {&brace: image(&brace), &empty: image(&empty)}
                ])
            );
            assert_eq!(
                latex_finished(&mut rt, root),
                [reported(&brace), reported(&brace), reported(&empty)]
            );
        }
        assert_eq!(svgs(&out), [], "MicroTeX made an image");
    }
}

/// Stopping a render kills MicroTeX at once; the job blocks on the child instead of polling.
#[test]
fn latex_stop_kills_microtex() {
    let d = tmp("latex-stop");
    let started = d.join("started");
    assert!(
        Command::new("mkfifo")
            .arg(&started)
            .status()
            .unwrap()
            .success()
    );
    // Reports that it runs through the FIFO, then blocks until killed: a run that waited for
    // MicroTeX to exit on its own never returns (nextest's slow-timeout ends it).
    testenv::install_script(
        &d.join("LaTeX"),
        &format!(
            "#!/bin/sh\necho > '{}'\nexec sleep infinity\n",
            started.display()
        ),
    );
    let stop = std::sync::Arc::new(qml::svc::Stop::default());
    let s = stop.clone();
    std::thread::spawn(move || {
        std::fs::read(&started).unwrap();
        s.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let (code, _, _) = qml::latex::run(&d.to_string_lossy(), "LaTeX", &[], &stop);
    // 128 + SIGTERM, as bash reports a terminated child: Stop signals MicroTeX's whole process
    // group (crate::child::run), so a helper it forked dies too.
    assert_eq!(code, 128 + libc::SIGTERM);
}
