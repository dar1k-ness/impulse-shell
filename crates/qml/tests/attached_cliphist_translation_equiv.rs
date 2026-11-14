//! AttachedFileIndicator.qml `fileTypeProc` and `imageSizeProc` (`Impulse` types `FileMimeType`
//! and `ImageSize`) and Translation.qml's `TranslationScanner` (`Impulse` type
//! `TranslationLanguages`): the native side leaves the QML in the state the original's own commands, run
//! by the real `file`, `identify` and `find`, leave it in, except where the original's result is wrong.
//! The tree's QML is loaded verbatim. The reference side of an `Impulse` type is the `Process`
//! The original had there, kept in this file.
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use testenv::Wakeups;

static ENV: Mutex<()> = Mutex::new(());

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

fn tmp(n: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("act-equiv-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The `<head> { … }` block of the original's `rel` that starts at the first `head` after `after`.
fn block_at(rel: &str, after: &str, head: &str) -> String {
    // AttachedFileIndicator.qml: QML the shell no longer loads, kept for this comparison.
    let tree = if rel == ATTACHED {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/original-unshipped")
    } else {
        ri()
    };
    let src = std::fs::read_to_string(tree.join(rel)).unwrap();
    let at = src
        .find(after)
        .unwrap_or_else(|| panic!("{after} not in {rel}"));
    let start = if head.is_empty() {
        at
    } else {
        src[..at + after.len()].rfind(head).unwrap()
    };
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
    panic!("unterminated block in {rel}")
}

/// A runtime whose waker and process fd (`Runtime::process_fd`, before any process starts,
/// so each one is watched) report to the returned [`Wakeups`] for [`pump`].
fn runtime() -> (Runtime, Wakeups) {
    let mut rt = Runtime::new(Some(ri()));
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    rt.process_fd();
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
        &|n| !services.iter().any(|s| s == n),
    );
    rt.settle();
    (rt, wake)
}

/// Loads `body` as the original's `rel` (inside a scratch tree, so the job lookup sees the original's file name).
fn load(rt: &mut Runtime, tree: &Path, rel: &str, body: &str) -> usize {
    let f = tree.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    let head = "import Impulse\nimport QtQuick\nimport Quickshell\nimport Rustshell.Io\nimport ri.modules.common\nimport ri.modules.common.functions\n";
    std::fs::write(&f, format!("{head}{body}")).unwrap();
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    root
}

fn read(rt: &mut Runtime, item: usize, name: &str) -> Value {
    rt.world.borrow_mut().read(item, name).unwrap_or_default()
}

fn num(rt: &mut Runtime, item: usize, name: &str) -> f64 {
    match read(rt, item, name) {
        Value::Num(n) => n,
        v => panic!("{name}: {v:?}"),
    }
}

fn json(rt: &mut Runtime, item: usize, f: &str) -> String {
    rt.call_method(item, f, vec![])
        .unwrap()
        .unwrap()
        .to_js_string()
}

/// Runs the shell's events until `done` holds and nothing it started is left: no process
/// running, no pool job, no wakeup pending. Blocks on the wakeups in between (a hang is
/// nextest's slow-timeout to end).
fn pump(rt: &mut Runtime, wake: &Wakeups, done: &dyn Fn(&mut Runtime) -> bool) {
    let ep = rt.process_fd();
    loop {
        workpool::wait_idle();
        rt.poll_native();
        rt.settle();
        let idle = done(rt) && rt.world.borrow().procs.is_empty();
        let woke = wake.wait(ep, idle.then_some(Duration::ZERO));
        if woke.fd {
            rt.process_fd_ready();
        }
        if idle && !woke.any() {
            return;
        }
    }
}

const ATTACHED: &str = "modules/rusty/sidebarLeft/aiChat/AttachedFileIndicator.qml";

/// `refresh()`, `fileTypeProc` and `imageSizeProc` as the original had them: `file` and `identify`.
const ATTACHED_COMMANDS: [&str; 3] = [
    r#"function refresh() {
        root.mimeType = "";
        root.imageWidth = -1;
        root.imageHeight = -1;
        fileTypeProc.exec(["file", "-b", "--mime-type", filePath]);
    }"#,
    r#"Process {
        id: fileTypeProc
        command: ["file", "-b", "--mime-type", filePath]
        stdout: StdioCollector {
            onStreamFinished: {
                root.mimeType = this.text;
                if (root.mimeType.startsWith("image/"))
                    imageSizeProc.exec(["identify", "-format", "%wx%h", filePath]);
            }
        }
    }"#,
    r#"Process {
        id: imageSizeProc
        command: ["identify", "-format", "%wx%h", filePath]
        stdout: StdioCollector {
            onStreamFinished: {
                const dimensions = this.text.split("x");
                root.imageWidth = parseInt(dimensions[0]);
                root.imageHeight = parseInt(dimensions[1]);
            }
        }
    }"#,
];

/// AttachedFileIndicator's `refresh()` → `fileTypeProc` (→ `imageSizeProc` for images) for
/// each path; per path `[mimeType, imageWidth, imageHeight]`.
fn attached_run(native: bool, tree: &Path, paths: &[String]) -> Vec<(String, f64, f64)> {
    let [refresh, file_type, image_size] = if native {
        [
            block_at(ATTACHED, "function refresh() {", ""),
            block_at(ATTACHED, "id: fileTypeProc\n", "FileMimeType {"),
            block_at(ATTACHED, "id: imageSizeProc\n", "ImageSize {"),
        ]
    } else {
        ATTACHED_COMMANDS.map(String::from)
    };
    let body = format!(
        "Item {{
    id: root
    property string filePath: \"\"
    property string mimeType: \"\"
    property real imageWidth: -1
    property real imageHeight: -1
    property int typed: 0
    property int sized: 0
    {refresh}
    Connections {{ target: fileTypeProc; function onExited(c, s) {{ root.typed++ }} }}
    Connections {{ target: imageSizeProc; function onExited(c, s) {{ root.sized++ }} }}
    {file_type}
    {image_size}
}}
"
    );
    let (mut rt, wake) = runtime();
    let root = load(&mut rt, tree, ATTACHED, &body);
    let mut out = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        rt.set(root, "filePath", Value::Str(p.as_str().into()));
        let sized = num(&mut rt, root, "sized");
        rt.call_method(root, "refresh", vec![]);
        let want = (i + 1) as f64;
        pump(&mut rt, &wake, &|rt| num(rt, root, "typed") >= want);
        if read(&mut rt, root, "mimeType")
            .to_js_string()
            .starts_with("image/")
        {
            pump(&mut rt, &wake, &|rt| num(rt, root, "sized") > sized);
        }
        out.push((
            read(&mut rt, root, "mimeType").to_js_string(),
            num(&mut rt, root, "imageWidth"),
            num(&mut rt, root, "imageHeight"),
        ));
    }
    out
}

#[test]
fn attached_file_indicator_file_type_proc_matches_the_original() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    if !testenv::require_all(&["file", "identify", "magick"]) {
        return;
    }
    let d = tmp("mime");
    let f = d.join("files");
    std::fs::create_dir_all(f.join("a dir")).unwrap();
    let made = std::process::Command::new("magick")
        .args(["-size", "3x2", "xc:red"])
        .arg(f.join("pic.png"))
        .status()
        .unwrap();
    assert!(made.success());
    std::fs::write(
        f.join("doc.pdf"),
        b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n1 0 obj\n<<>>\nendobj\n",
    )
    .unwrap();
    std::fs::write(f.join("notes $x 'q'.txt"), "hello world\n").unwrap();
    std::fs::write(f.join("empty"), b"").unwrap();
    std::fs::write(f.join("data.json"), "{\"a\": 1}\n").unwrap();
    std::os::unix::fs::symlink(f.join("pic.png"), f.join("link.png")).unwrap();
    std::os::unix::fs::symlink(f.join("nowhere"), f.join("dangling")).unwrap();
    let names = [
        "pic.png",
        "doc.pdf",
        "notes $x 'q'.txt",
        "empty",
        "data.json",
        "a dir",
        "link.png",
        "dangling",
        "missing",
    ];
    let paths: Vec<String> = names
        .iter()
        .map(|n| f.join(n).to_string_lossy().into_owned())
        .collect();
    let orig = attached_run(false, &d.join("o"), &paths);
    let nat = attached_run(true, &d.join("n"), &paths);
    for (i, n) in names.iter().enumerate() {
        let (o, v) = (&orig[i], &nat[i]);
        match *n {
            // The original: `inode/symlink`, so an attached link to a picture showed no picture.
            "link.png" => {
                assert_eq!(o.0, "inode/symlink\n");
                assert_eq!(v, &nat[0], "{n}");
            }
            // The original: `file`'s "cannot open `…'" error text as the MIME type.
            "dangling" | "missing" => {
                let ri = if *n == "dangling" {
                    "inode/symlink\n"
                } else {
                    "cannot open"
                };
                assert!(o.0.starts_with(ri), "{n}: {o:?}");
                assert_eq!(v, &(String::new(), -1.0, -1.0), "{n}");
            }
            // The original keeps `file`'s newline, so `mimeType === "application/pdf"` (the PDF icon)
            // never held; the type itself is the same.
            _ => {
                assert_eq!(format!("{}\n", v.0), o.0, "{n}");
                assert_eq!((v.1, v.2), (o.1, o.2), "{n}");
            }
        }
    }
    assert_eq!(nat[0], ("image/png".into(), 3.0, 2.0));
    assert_eq!(nat[1].0, "application/pdf");
}

const TRANSLATION: &str = "services/Translation.qml";

/// `TranslationScanner` as the original had it: `find` and `basename`.
const SCANNER_COMMAND: &str = r#"component TranslationScanner: Process {
        id: translationScanner
        required property string translationsDir
        signal languagesScanned(var languages)

        command: ["find", translationScanner.translationsDir, "-name", "*.json", "-exec", "basename", "{}", ".json", ";"]
        running: true

        stdout: StdioCollector {
            id: languagesCollector
            onStreamFinished: {
                const output = languagesCollector.text;
                const files = output.trim().split('\n').map(f => f.trim());
                translationScanner.languagesScanned(files);
            }
        }

        onExited: (exitCode, exitStatus) => {
            if (exitCode !== 0) {
                translationScanner.languagesScanned(["en_US"]);
            }
        }
    }"#;

/// Translation.qml's two `TranslationScanner`s (the component verbatim) over `dirs`, and
/// `allAvailableLanguages`.
fn translation_run(native: bool, tree: &Path, dirs: (&Path, &Path)) -> String {
    let scanner = if native {
        block_at(
            TRANSLATION,
            "component TranslationScanner: TranslationLanguages {",
            "",
        )
    } else {
        SCANNER_COMMAND.to_string()
    };
    let all = block_at(TRANSLATION, "property var allAvailableLanguages: {", "");
    let first = block_at(
        TRANSLATION,
        "id: scanLanguagesProcess",
        "TranslationScanner {",
    );
    let second = block_at(
        TRANSLATION,
        "id: scanGeneratedLanguagesProcess",
        "TranslationScanner {",
    );
    let body = format!(
        "Item {{
    id: root
    property var availableLanguages: [\"en_US\"]
    property var availableGeneratedLanguages: []
    {all}
    property string translationsDir: {:?}
    property string generatedTranslationsDir: {:?}
    property int scanned: 0
    Connections {{ target: scanLanguagesProcess; function onExited(c, s) {{ root.scanned++ }} }}
    Connections {{ target: scanGeneratedLanguagesProcess; function onExited(c, s) {{ root.scanned++ }} }}
    function state() {{ return JSON.stringify([root.availableLanguages, root.availableGeneratedLanguages, root.allAvailableLanguages]) }}
    {first}
    {second}
    {scanner}
}}
",
        dirs.0.to_string_lossy(),
        dirs.1.to_string_lossy(),
    );
    let (mut rt, wake) = runtime();
    let root = load(&mut rt, tree, TRANSLATION, &body);
    pump(&mut rt, &wake, &|rt| num(rt, root, "scanned") >= 2.0);
    json(&mut rt, root, "state")
}

/// `[available, generated, all]` with each list sorted (find lists in directory order).
fn sorted(state: &str) -> Vec<Vec<String>> {
    let v: Vec<Vec<String>> = serde_json::from_str(state).unwrap();
    v.into_iter()
        .map(|mut l| {
            l.sort();
            l
        })
        .collect()
}

#[test]
fn translation_scanner_languages_collector_matches_the_original() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    if !testenv::require_all(&["find", "basename"]) {
        return;
    }
    let d = tmp("translation");
    let shipped = ri().join("translations");
    let empty = d.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let odd = d.join("odd dir");
    std::fs::create_dir_all(odd.join("tools")).unwrap();
    for f in [
        "xx_XX.json",
        "yy YY.json",
        "tools/nested.json",
        ".json",
        "notes.txt",
        "zz.json.bak",
    ] {
        std::fs::write(odd.join(f), "{}").unwrap();
    }
    std::fs::create_dir_all(odd.join("dir.json")).unwrap();
    let missing = d.join("missing");
    let cases: [(&str, &Path, &Path); 4] = [
        ("shipped+missing", &shipped, &missing),
        ("shipped+empty", &shipped, &empty),
        ("odd+shipped", &odd, &shipped),
        ("missing+missing", &missing, &missing),
    ];
    for (i, (name, a, b)) in cases.iter().enumerate() {
        let tree = d.join(format!("t{i}"));
        let o = sorted(&translation_run(false, &tree.join("o"), (a, b)));
        let n = sorted(&translation_run(true, &tree.join("n"), (a, b)));
        match *name {
            // The original: `[""]` for a directory without translations, an empty language in the list.
            "shipped+empty" => {
                assert_eq!(o[1], vec![String::new()]);
                assert_eq!(n[0], o[0]);
                assert!(n[1].is_empty(), "{n:?}");
                assert_eq!(n[2], o[0]);
            }
            // The original: also `nested`, `.json` and `dir` (a subdirectory's file, a nameless file,
            // a directory), which TranslationReader cannot load as `DIR/<code>.json`.
            "odd+shipped" => {
                assert_eq!(n[0], vec!["xx_XX", "yy YY"]);
                for x in [".json", "dir", "nested", "xx_XX", "yy YY"] {
                    assert!(o[0].iter().any(|l| l == x), "{x}: {o:?}");
                }
                assert_eq!(n[1], o[1]);
            }
            _ => assert_eq!(n, o, "{name}"),
        }
    }
}
