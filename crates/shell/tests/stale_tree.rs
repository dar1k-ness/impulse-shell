//! A QML tree edited or updated after the build: the scripts that differ from the compiled
//! tree have no compiled entry. The release shell runs them in the interpreter and keeps the
//! compiled code for the rest; `ricompiled::stale_files` names them for the AOT-only build,
//! which refuses to start rather than run without them, and refuses `Rustshell.reload()` too.

use std::path::{Path, PathBuf};

const PAIR: &str = "modules/common/models/AnimatedTabIndexPair.qml";
const UTILS: &str = "modules/common/functions/NumberUtils.qml";
const ADDED: &str = "modules/common/functions/added.js";

fn copy(from: &Path, to: &Path, key: &str, edit: impl FnOnce(String) -> String) {
    let src = std::fs::read_to_string(from.join(key)).unwrap();
    let dst = to.join(key);
    std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
    std::fs::write(dst, edit(src)).unwrap();
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

/// An edited copy of the QML tree, in a directory of its own per `test` (they run in
/// parallel). The whole tree: a reload is refused on one that lacks a compiled file too.
fn edited_tree(test: &str) -> PathBuf {
    let ri = PathBuf::from(ricompiled::ROOT);
    let tmp = std::env::temp_dir().join(format!("ri-stale-tree-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    copy_tree(&ri, &tmp);
    copy(&ri, &tmp, PAIR, |s| {
        let edited = s.replace(
            "property int idx2Duration: 300",
            "property int idx2Duration: root.idx1Duration * 4",
        );
        assert_ne!(edited, s, "{PAIR} changed; update the edit");
        edited
    });
    copy(&ri, &tmp, UTILS, |s| s);
    let added = tmp.join(ADDED);
    std::fs::write(added, "function added() { return 1; }\n").unwrap();
    tmp
}

#[test]
fn stale_files_names_only_what_differs() {
    let tmp = edited_tree("stale_files_names_only_what_differs");
    let src = std::fs::read_to_string(tmp.join(PAIR)).unwrap();
    let total = ricompiled::doc_scripts(PAIR, &qml::parse(&src).unwrap()).len();
    let stale = ricompiled::stale_files(&tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    let s = |file: &str, differ, total| ricompiled::Stale {
        file: file.into(),
        why: ricompiled::Why::Scripts { differ, total },
    };
    assert_eq!(stale, [s(PAIR, 1, total), s(ADDED, 1, 1)]);
}

/// The AOT-only shell re-executed on a stale tree would exit and leave no shell: reload must
/// not get as far as `exec`, and must leave the `Process` children alone.
#[test]
fn reload_on_stale_tree_keeps_the_running_shell() {
    let tmp = edited_tree("reload_on_stale_tree_keeps_the_running_shell");
    let mut execs = 0;
    let refused = shell::reload::reload(
        &tmp,
        None,
        true,
        || panic!("flushed for a reload refused as stale"),
        |_| {
            execs += 1;
            std::io::Error::other("exec")
        },
    );
    let edited = shell::reload::reload(
        &tmp,
        Some(&tmp.join(PAIR)),
        true,
        || panic!("flushed for a reload refused as stale"),
        |_| {
            execs += 1;
            std::io::Error::other("exec")
        },
    );
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(execs, 0, "reload re-executed on a stale tree");
    let shell::reload::Refused::Stale(stale) = refused else {
        panic!("not refused as stale: {refused:?}");
    };
    let files: Vec<_> = stale.iter().filter_map(|s| s.split(": ").next()).collect();
    assert_eq!(files, [PAIR, ADDED]);
    assert!(
        matches!(edited, shell::reload::Refused::Stale(ref s) if s.len() == 1 && s[0].starts_with(PAIR))
    );
    let body = shell::reload::stale_text(&tmp, &stale);
    assert!(body.contains(PAIR) && body.contains(ADDED), "{body}");
    assert!(body.contains(shell::reload::REBUILD_HINT), "{body}");
}

/// A changed file the build cannot read or parse has no scripts to match, so nothing shows
/// that the build runs it: the re-executed AOT-only shell would fail to load it.
#[test]
fn unreadable_or_unparsable_change_is_stale() {
    let tmp = edited_tree("unreadable_or_unparsable_change_is_stale");
    std::fs::remove_file(tmp.join(ADDED)).unwrap();
    let mut syntax = String::new();
    copy(Path::new(ricompiled::ROOT), &tmp, PAIR, |s| {
        let broken = s.replace(
            "property int idx2Duration: 300",
            "property int idx2Duration: (",
        );
        syntax = qml::parse(&broken).unwrap_err().to_string();
        broken
    });
    const GONE: &str = "modules/common/functions/Gone.qml";
    std::os::unix::fs::symlink(tmp.join("missing.qml"), tmp.join(GONE)).unwrap();
    let mut execs = 0;
    let refused = shell::reload::reload(
        &tmp,
        None,
        true,
        || panic!("flushed for a reload refused as stale"),
        |_| {
            execs += 1;
            std::io::Error::other("exec")
        },
    );
    let edited = shell::reload::reload(
        &tmp,
        Some(&tmp.join(PAIR)),
        true,
        || panic!("flushed for a reload refused as stale"),
        |_| {
            execs += 1;
            std::io::Error::other("exec")
        },
    );
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(execs, 0, "reload re-executed on a tree it cannot check");
    let shell::reload::Refused::Stale(stale) = refused else {
        panic!("not refused as stale: {refused:?}");
    };
    let [gone, pair] = &stale[..] else {
        panic!("{stale:?}");
    };
    assert!(
        gone.starts_with(&format!("{GONE}: cannot be read: ")),
        "{gone}"
    );
    assert_eq!(*pair, format!("{PAIR}: syntax error at {syntax}"));
    assert!(
        matches!(edited, shell::reload::Refused::Stale(ref s) if s == std::slice::from_ref(pair))
    );
    let body = shell::reload::stale_text(&tmp, &stale);
    assert!(
        body.contains(&format!("{PAIR}: syntax error at {syntax}")),
        "{body}"
    );
    assert!(
        body.contains(&format!("{GONE}: cannot be read: ")),
        "{body}"
    );
}

#[test]
fn reload_on_unchanged_tree_execs() {
    let tmp = edited_tree("reload_on_unchanged_tree_execs");
    std::fs::copy(Path::new(ricompiled::ROOT).join(PAIR), tmp.join(PAIR)).unwrap();
    std::fs::remove_file(tmp.join(ADDED)).unwrap();
    let steps = std::cell::RefCell::new(Vec::new());
    let r = shell::reload::reload(
        &tmp,
        None,
        true,
        || steps.borrow_mut().push("flush"),
        |_| {
            steps.borrow_mut().push("exec");
            std::io::Error::other("exec")
        },
    );
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(steps.into_inner(), ["flush", "exec"]);
    assert!(matches!(r, shell::reload::Refused::Exec(_)), "{r:?}");
}

#[cfg(feature = "interp")]
#[test]
fn edited_script_runs_beside_compiled_ones() {
    use qml::runtime::Runtime;
    let tmp = edited_tree("edited_script_runs_beside_compiled_ones");
    let mut rt = Runtime::new(Some(tmp.clone()));
    rt.eng()
        .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&tmp))));
    qml::jsrt::track_runs(true);
    let root = rt.load_file_init(&tmp.join(PAIR), &|_, v| Some(v.clone()));
    let ran = qml::jsrt::take_runs();
    qml::jsrt::track_runs(false);
    let _ = std::fs::remove_dir_all(&tmp);
    let root = root.unwrap();
    assert!(
        !ran.is_empty(),
        "the unedited bindings did not run compiled code"
    );
    assert_eq!(rt.get(root, "idx1Duration").to_number(), 100.0);
    assert_eq!(
        rt.get(root, "idx2Duration").to_number(),
        400.0,
        "the edited binding did not run"
    );
}
