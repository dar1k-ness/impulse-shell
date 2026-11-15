//! Tests wait on events (a child's output, its pidfd, the server's log line for a registered
//! listener, a presented frame, a protocol round trip, a channel, the runtime's waker and
//! epoll set), never on the clock: no sleep, no `try_wait` loop, no call retried until a
//! deadline passes. A poll is slow on a fast machine and flaky on a slow one, and a fixed sleep
//! before an action races the child it waits for. nextest's slow-timeout
//! (`.config/nextest.toml`) is the only guard against a test that hangs.
//!
//! Scanned: every file under `crates/*/tests` (Rust, and the scripts, configs and step lists
//! the tests run), the `#[cfg(test)]` modules under `crates/*/src` with the files they load,
//! the test support crate `testenv`, and every tool under `crates/*/src/bin` (the headless
//! drivers, floods, scene dumps and manual checks that run the runtime for equivalence diffs
//! and measurements). The code under test may wait with a timeout where that is its job (the
//! pool's delayed jobs, idle exits), so only its test modules are scanned.

use std::path::{Path, PathBuf};

/// Scanned whole besides the test trees and the tools: test support.
const WHOLE: &[&str] = &["crates/testenv/src", "crates/rigen/check/src"];

/// The original's own scripts, kept verbatim as what the comparison tests compare with: not test code.
const ORIGINAL_SCRIPTS: &str = "crates/qml/tests/fixtures/original-unshipped/scripts";

/// What a wall-clock poll or a fixed wait is written with, as substrings.
const POLLS: &[&str] = &[
    "thread::sleep",
    "Timer::after",
    "Timer::at",
    "Timer::interval",
    ".try_wait()",
    "recv_timeout(",
    "recv_deadline(",
    "wait_timeout(",
    "wait_timeout_while(",
    "wait_timeout_ms(",
    "park_timeout(",
    "sleep_until(",
    "set_read_timeout(",
    "set_write_timeout(",
    "Instant::now() <",
    "Instant::now() >",
    ".elapsed() <",
    ".elapsed() >",
    "usleep(",
    "nanosleep(",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A `sleep` command or call with a duration: `sleep 0.3`, `sleep(1)`, `time.sleep(t)`,
/// `sleep $T`, a step list's `sleep 1500`. `sleep infinity`, a fake that blocks until it is
/// killed, waits on no clock.
fn timed_sleep(line: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    line.match_indices("sleep").any(|(i, _)| {
        if line[..i].chars().next_back().is_some_and(ident) {
            return false;
        }
        let rest = &line[i + "sleep".len()..];
        if let Some(arg) = rest.strip_prefix('(') {
            // A call; `sleep()` with no argument is not one that waits on a duration.
            return !arg.starts_with(')');
        }
        let arg = rest.trim_start();
        arg.len() < rest.len()
            && arg
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || c == '.' || c == '$')
    })
}

/// `line` with a Rust line comment dropped (one not inside a URL's `://`).
fn code(line: &str) -> &str {
    let mut from = 0;
    while let Some(i) = line[from..].find("//") {
        let at = from + i;
        if !line[..at].ends_with(':') {
            return &line[..at];
        }
        from = at + 2;
    }
    line
}

fn polls(line: &str) -> bool {
    POLLS.iter().any(|p| line.contains(p)) || timed_sleep(line)
}

/// Every file under `p`, or `p` itself.
fn files(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let mut es: Vec<PathBuf> = std::fs::read_dir(p)
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
            .map(|e| e.unwrap().path())
            .collect();
        es.sort();
        for e in es {
            files(&e, out);
        }
    } else {
        out.push(p.to_path_buf());
    }
}

/// Whether an attribute line compiles its item for tests only: `#[cfg(test)]`,
/// `#[cfg(all(test, feature = "interp"))]`, not `#[cfg(not(test))]`.
fn test_only(attr: &str) -> bool {
    attr.starts_with("#[cfg(")
        && !attr.contains("not(test")
        && !attr.contains("any(")
        && attr
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .any(|w| w == "test")
}

/// Which bytes of the Rust source `src` are code: `false` for the bytes of a comment and of
/// a string, byte string, C string, raw string or character literal (the Reference,
/// "Comments" and "Tokens"), so a brace or an attribute inside one opens and closes nothing.
fn code_bytes(src: &str) -> Vec<bool> {
    let b = src.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || !c.is_ascii();
    let mut code = vec![true; b.len()];
    let mut i = 0;
    while i < b.len() {
        let end = match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => i + src[i..].find('\n').unwrap_or(b.len() - i),
            // Block comments nest.
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let (mut k, mut depth) = (i + 2, 1);
                while k < b.len() && depth > 0 {
                    match &b[k..(k + 2).min(b.len())] {
                        b"/*" => (k, depth) = (k + 2, depth + 1),
                        b"*/" => (k, depth) = (k + 2, depth - 1),
                        _ => k += 1,
                    }
                }
                k
            }
            b'"' => {
                let mut k = i + 1;
                while k < b.len() && b[k] != b'"' {
                    k += if b[k] == b'\\' { 2 } else { 1 };
                }
                (k + 1).min(b.len())
            }
            // `'x'` and `'\n'` are characters; `'a` is a lifetime or a label.
            b'\'' => {
                let one = src[i + 1..].chars().next().map_or(0, char::len_utf8);
                if b.get(i + 1) == Some(&b'\\') {
                    src.get(i + 3..)
                        .and_then(|s| s.find('\''))
                        .map_or(b.len(), |n| i + 3 + n + 1)
                } else if one > 0 && b.get(i + 1 + one) == Some(&b'\'') {
                    i + one + 2
                } else {
                    i += 1;
                    continue;
                }
            }
            c if word(c) => {
                let mut e = i;
                while e < b.len() && word(b[e]) {
                    e += 1;
                }
                // `r"…"`, `br#"…"#`, `cr##"…"##`: no escapes, closed by the quote and as
                // many `#` as opened it. `r#name` is a raw identifier.
                let hashes = b[e..].iter().take_while(|&&c| c == b'#').count();
                if !matches!(&b[i..e], b"r" | b"br" | b"cr") || b.get(e + hashes) != Some(&b'"') {
                    i = e;
                    continue;
                }
                let from = e + hashes + 1;
                let close = format!("\"{}", "#".repeat(hashes));
                src[from..]
                    .find(&close)
                    .map_or(b.len(), |n| from + n + close.len())
            }
            _ => {
                i += 1;
                continue;
            }
        };
        code[i..end].fill(false);
        i = end;
    }
    code
}

/// Where each line of `src` starts, as `str::lines` splits it.
fn line_starts(src: &str) -> Vec<usize> {
    src.split_inclusive('\n')
        .scan(0, |at, l| Some(std::mem::replace(at, *at + l.len())))
        .collect()
}

/// The line of the `}` that closes the block the last `{` of line `open` opens: braces are
/// counted as Rust reads them, those of literals and comments (`code`, from [`code_bytes`])
/// are not. `starts` is [`line_starts`].
fn closing_line(src: &str, code: &[bool], starts: &[usize], open: usize) -> Option<usize> {
    let b = src.as_bytes();
    let next = starts.get(open + 1).map_or(b.len(), |s| *s);
    let from = (starts[open]..next).rfind(|&k| b[k] == b'{' && code[k])?;
    let mut depth = 0_usize;
    for k in (from..b.len()).filter(|&k| code[k]) {
        match b[k] {
            b'{' => depth += 1,
            b'}' if depth == 1 => return Some(starts.partition_point(|&s| s <= k) - 1),
            b'}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The test modules of the source file `f`: inline ones as line ranges of `src`, and the
/// files of out-of-line ones.
fn test_modules(f: &Path, src: &str) -> (Vec<std::ops::Range<usize>>, Vec<PathBuf>) {
    let lines: Vec<&str> = src.lines().collect();
    // Where `mod x;` is looked up (the Reference, "Modules"), and `#[path]` is relative to.
    let here = f.parent().unwrap();
    let stem = f.file_stem().unwrap().to_str().unwrap();
    let dir = if matches!(stem, "mod" | "lib" | "main") {
        here.to_path_buf()
    } else {
        here.join(stem)
    };
    let (code, starts) = (code_bytes(src), line_starts(src));
    let (mut inline, mut outline) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < lines.len() {
        let attr = lines[i].trim_start();
        // An attribute written in a string or a comment marks nothing.
        if !test_only(attr) || !code[starts[i] + lines[i].len() - attr.len()] {
            i += 1;
            continue;
        }
        let mut path = None;
        let mut j = i + 1;
        while j < lines.len() && lines[j].trim_start().starts_with("#[") {
            if let Some(p) = lines[j].trim().strip_prefix("#[path = \"") {
                path = p.strip_suffix("\"]").map(str::to_owned);
            }
            j += 1;
        }
        let Some(item) = lines.get(j).map(|l| l.trim_start()) else {
            break;
        };
        let item = item
            .strip_prefix("pub(crate) ")
            .or_else(|| item.strip_prefix("pub(super) "))
            .or_else(|| item.strip_prefix("pub "))
            .unwrap_or(item);
        if let Some(m) = item.strip_prefix("mod ") {
            if let Some(name) = m.strip_suffix(';') {
                let file = match path {
                    Some(p) => here.join(p),
                    None => [
                        dir.join(format!("{name}.rs")),
                        dir.join(name).join("mod.rs"),
                    ]
                    .into_iter()
                    .find(|p| p.exists())
                    .unwrap_or_else(|| panic!("{}: no file for `mod {name};`", f.display())),
                };
                // Its own submodules are test code too.
                let sub = file.with_extension("");
                if file.file_name().is_some_and(|n| n == "mod.rs") {
                    files(file.parent().unwrap(), &mut outline);
                } else {
                    outline.push(file);
                    if sub.is_dir() {
                        files(&sub, &mut outline);
                    }
                }
            } else if m.ends_with('{') {
                let end = closing_line(src, &code, &starts, j)
                    .unwrap_or_else(|| panic!("{}:{}: unclosed module", f.display(), j + 1));
                inline.push(j..end + 1);
                i = end + 1;
                continue;
            }
        }
        i = j + 1;
    }
    (inline, outline)
}

#[test]
fn tests_wait_on_events_not_on_the_clock() {
    let root = root();
    let mut whole = Vec::new();
    let mut units = Vec::new();
    for c in std::fs::read_dir(root.join("crates")).unwrap() {
        let c = c.unwrap().path();
        if c.join("tests").is_dir() {
            files(&c.join("tests"), &mut whole);
        }
        if c.join("src").is_dir() {
            files(&c.join("src"), &mut units);
        }
        // Cargo's auto-discovered binaries and the `[[bin]]` paths all live here.
        if c.join("src/bin").is_dir() {
            files(&c.join("src/bin"), &mut whole);
        }
    }
    for w in WHOLE {
        files(&root.join(w), &mut whole);
    }
    let me = Path::new(file!()).file_name().unwrap();
    let mut scanned = Vec::new();
    for f in units
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "rs"))
    {
        let src = std::fs::read_to_string(f).unwrap();
        let (inline, outline) = test_modules(f, &src);
        whole.extend(outline);
        if !inline.is_empty() {
            scanned.push((f.clone(), src, inline));
        }
    }
    whole.sort();
    whole.dedup();
    let ri_scripts = root.join(ORIGINAL_SCRIPTS);
    for f in whole
        .into_iter()
        .filter(|f| f.file_name() != Some(me) && !f.starts_with(&ri_scripts))
    {
        // Fixtures that are not text (images, fonts) hold no waits.
        if let Ok(src) = std::fs::read_to_string(&f) {
            let n = src.lines().count();
            #[expect(
                clippy::single_range_in_vec_init,
                reason = "one line range: the whole file"
            )]
            scanned.push((f, src, vec![0..n]));
        }
    }
    let mut found = Vec::new();
    for (f, src, ranges) in &scanned {
        let rust = f.extension().is_some_and(|e| e == "rs");
        let lines: Vec<&str> = src.lines().collect();
        for r in ranges {
            for (k, line) in lines[r.clone()].iter().enumerate() {
                let line = if rust { code(line) } else { line };
                if polls(line) {
                    let f = f.strip_prefix(&root).unwrap_or(f);
                    found.push(format!(
                        "{}:{}: {}",
                        f.display(),
                        r.start + k + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        scanned.len() > 200,
        "scanned only {} files: is the tree there?",
        scanned.len()
    );
    assert!(found.is_empty(), "wall-clock waits:\n{}", found.join("\n"));
}

#[test]
fn the_scan_sees_each_way_of_waiting_on_the_clock() {
    for l in [
        "std::thread::sleep(Duration::from_millis(5));",
        "sleep(d);",
        "async_io::Timer::after(d).await;",
        "while c.try_wait().unwrap().is_none() {",
        "rx.recv_timeout(Duration::from_secs(3))",
        "while Instant::now() < end {",
        "if t.elapsed() > Duration::from_secs(10) {",
        "sleep 0.3",
        "  sleep 1.5",
        "auth required pam_exec.so /bin/sleep 60",
        "list: sleep 1500",
        "time.sleep(0.1)",
        "sleep $DELAY",
        "usleep(1000);",
    ] {
        assert!(polls(l), "{l}");
    }
    for l in [
        "exec -a \"$0\" sleep infinity",
        "wait_exit(&mut c); // no sleep, no try_wait",
        "let asleep = 1;",
        "fn sleep_inhibited() {",
    ] {
        assert!(!polls(code(l)), "{l}");
    }
}

#[test]
fn test_modules_are_found_inline_out_of_line_and_by_path() {
    let src = "fn f() {}\n\n#[cfg(test)]\nmod tests {\n    fn g() {\n    }\n}\n\nfn h() {}\n\
               #[cfg(all(test, feature = \"interp\"))]\n#[path = \"x_tests.rs\"]\nmod px;\n\
               #[cfg(not(test))]\nmod real {\n}\n";
    let root = root();
    let f = root.join("crates/shell/src/a.rs");
    let (inline, outline) = test_modules(&f, src);
    #[expect(
        clippy::single_range_in_vec_init,
        reason = "one line range: the module's"
    )]
    let want = [3..7];
    assert_eq!(inline, want);
    assert_eq!(outline, [root.join("crates/shell/src/x_tests.rs")]);
}

/// A source file whose literals and comments hold what the scan looks for: a `}` at the
/// start of a line inside the test module, and a `#[cfg(test)]` module in a string of the
/// production code.
const LITERALS: &str = r##"fn before() {}

#[cfg(test)]
mod tests {
    const DOC: &str = r#"Item {
    Timer { interval: 5 }
}
"#;
    const TEXT: &str = "\"
}
";
    /* a block comment
}
    /* a nested one */
}
    */
    // }
    fn braces() -> [char; 3] {
        ['}', '{', '\'']
    }
    fn waits<'a>(rx: &'a Receiver<()>) {
        rx.recv_timeout(Duration::from_secs(30)).unwrap();
    }
}

fn after() {}
const NOT_A_MODULE: &str = r#"
#[cfg(test)]
mod fake {
"#;
fn clock() {
    std::thread::sleep(LIMIT);
}
"##;

#[test]
fn literals_and_comments_are_not_code() {
    let code = |src: &str| -> String {
        let code = code_bytes(src);
        src.char_indices()
            .filter(|&(i, _)| code[i])
            .map(|(_, c)| c)
            .collect()
    };
    for (src, want) in [
        (r#"a("x}\"y\\", b"{", c"}") z"#, "a(, b, c) z"),
        (
            r###"r"\" + r#"x"}"# + br##"}"#"## + cr#"{"# z"###,
            " +  +  +  z",
        ),
        (
            r"f('}', '\'', b'{', '\u{7d}', 'é', '\\') z",
            "f(, , b, , , ) z",
        ),
        (
            "fn f<'a>(x: &'a str) { 'l: loop {} }",
            "fn f<'a>(x: &'a str) { 'l: loop {} }",
        ),
        ("let r#fn = r; x(r#fn, br)", "let r#fn = r; x(r#fn, br)"),
        (
            "x /* a /* } */ } */ y // }\nz /// {\n//! {\nw",
            "x  y \nz \n\nw",
        ),
        ("\"\n}\n\" a 'x", " a 'x"),
        ("a \"never closed }", "a "),
    ] {
        assert_eq!(code(src), want, "{src}");
    }
}

#[test]
fn a_test_module_ends_where_rust_ends_it_not_at_a_brace_in_a_literal_or_comment() {
    let lines: Vec<&str> = LITERALS.lines().collect();
    let at = |has: &str| lines.iter().position(|l| l.contains(has)).unwrap();
    let (inline, outline) = test_modules(&root().join("crates/shell/src/a.rs"), LITERALS);
    let test = |k: usize| inline.iter().any(|r| r.contains(&k));
    assert!(
        test(at("recv_timeout")),
        "a test's wait is taken for production code: test modules {inline:?}"
    );
    assert!(
        !test(at("sleep(LIMIT)")),
        "a production clock is taken for test code: test modules {inline:?}"
    );
    #[expect(
        clippy::single_range_in_vec_init,
        reason = "one line range: the module's"
    )]
    let want = [at("mod tests {")..at("fn after()") - 1];
    assert_eq!(inline, want);
    assert!(outline.is_empty());
}

/// Shutdown and lifecycle code that must wait on events only, not just in its tests: `(file
/// under crates/, the line its fn opens with)`, scanned to the fn's closing brace. A reload
/// hands the children over and execs at once, the next image kills them (their exits are
/// waited for nowhere: it reaps them), a flush waits for the writer's answer, a stopped job's PipeWire wait ends on
/// the stop.
const FUNCTIONS: &[(&str, &str)] = &[
    (
        "qml/src/native.rs",
        "pub fn hand_over_children() -> Vec<i32> {",
    ),
    (
        "qml/src/native.rs",
        "pub(super) fn hand_over() -> Vec<i32> {",
    ),
    (
        "qml/src/native.rs",
        "pub fn end_handed_over(groups: &[i32], inherited: &[i32]) {",
    ),
    (
        "qml/src/native.rs",
        "fn each_held(mut f: impl FnMut(i32) -> bool) {",
    ),
    (
        "qml/src/native.rs",
        "fn signal_all(sig: i32, mut then: impl FnMut(i32)) {",
    ),
    ("qml/src/hyprconf.rs", "pub fn flush() {"),
    (
        "qml/src/pw.rs",
        "pub fn wait_snapshot_or_stop(stop: &Arc<crate::svc::Stop>) -> Option<Snap> {",
    ),
];

/// The lines of the fn in `src` that opens with `open` (trimmed), through its closing brace.
fn function<'a>(name: &str, src: &'a str, open: &str) -> Vec<(usize, &'a str)> {
    let lines: Vec<&str> = src.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.trim() == open)
        .unwrap_or_else(|| panic!("{name} has no `{open}`"));
    let end = closing_line(src, &code_bytes(src), &line_starts(src), at)
        .unwrap_or_else(|| panic!("{name}: `{open}` never closes"));
    (at..=end).map(|i| (i, lines[i])).collect()
}

#[test]
fn shutdown_paths_have_no_fixed_waits() {
    let crates = root().join("crates");
    let found: Vec<String> = FUNCTIONS
        .iter()
        .flat_map(|&(name, open)| {
            let src = std::fs::read_to_string(crates.join(name)).unwrap();
            function(name, &src, open)
                .into_iter()
                .filter(|(_, l)| {
                    polls(l)
                        || ["Duration", "Instant", "poll("]
                            .iter()
                            .any(|p| l.contains(p))
                })
                .map(|(i, l)| format!("{name}:{}: {}", i + 1, l.trim()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(found.is_empty(), "fixed waits:\n{}", found.join("\n"));
}

/// The names the stress and bench scripts stop: the shells and the compositor.
const STOPPED: &[&str] = &["qs", "quickshell", "impulse-shell", "Hyprland", "\"$n\""];

/// `line` sends a signal to a shell or the compositor (`pkill -x NAME`), or to the shell by its
/// pid (`kill -TERM $p`).
fn stops_a_shell(line: &str) -> bool {
    let by_name = line.match_indices("pkill ").any(|(i, _)| {
        let args = line[i + "pkill ".len()..].split(';').next().unwrap_or("");
        args.split_whitespace().any(|a| STOPPED.contains(&a))
    });
    by_name || line.contains("kill -TERM $")
}

/// What is wrong with how the script `text` waits for the shell it stops, per line.
fn unwaited_stops(text: &str) -> Vec<(usize, &'static str)> {
    let waits = text.contains("pidwait");
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim_start().starts_with('#'))
        .filter_map(|(i, l)| {
            let why = if l.contains("-d /proc/$") && timed_sleep(l) {
                "polls /proc for a process"
            } else if stops_a_shell(l) && timed_sleep(l) {
                "sleeps a fixed time beside the signal"
            } else if stops_a_shell(l) && !waits {
                "nothing waits for the exit (pidwait)"
            } else {
                return None;
            };
            Some((i, why))
        })
        .collect()
}

/// The stress and bench scripts stop a shell (or the compositor under it) and go on once it
/// has exited: `pidwait` (pidfd_open(2) and poll(2) in procps-ng 4), as `stop_shell` of
/// `tools/stress/common.sh` does, never a fixed `sleep` followed by SIGKILL, which kills a
/// slow shell while it is still saving and makes a fast one wait, nor a loop over `/proc`.
#[test]
fn stress_and_bench_scripts_wait_for_the_exit_of_what_they_stop() {
    let mut scripts = Vec::new();
    for dir in ["tools/stress", "bench"] {
        files(&root().join(dir), &mut scripts);
    }
    scripts.retain(|f| f.extension().is_some_and(|e| e == "sh"));
    assert!(scripts.len() > 30, "scripts found: {}", scripts.len());
    let found: Vec<String> = scripts
        .iter()
        .flat_map(|f| {
            let text = std::fs::read_to_string(f).unwrap();
            let name = f.strip_prefix(root()).unwrap().display().to_string();
            unwaited_stops(&text)
                .into_iter()
                .map(|(i, why)| format!("{name}:{}: {why}", i + 1))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(found.is_empty(), "\n{}", found.join("\n"));
}

#[test]
fn the_scan_sees_each_way_a_script_does_not_wait_for_an_exit() {
    let whys = |text: &str| -> Vec<&'static str> {
        let found = unwaited_stops(text);
        found.into_iter().map(|(_, why)| why).collect()
    };
    assert_eq!(
        whys(
            "pkill -x qs; pkill -x impulse-shell; sleep 1.5; pkill -9 -x impulse-shell\n# pidwait"
        ),
        ["sleeps a fixed time beside the signal"]
    );
    assert_eq!(
        whys("kill -TERM $p; for w in 1 2; do [ -d /proc/$p ] || break; sleep 0.25; done"),
        ["polls /proc for a process"]
    );
    assert_eq!(
        whys("stop_shell() {\n    pkill -x quickshell || true\n    wait_for 20 no_layer bar\n}"),
        ["nothing waits for the exit (pidwait)"]
    );
    assert_eq!(
        whys("hyprctl dispatch exit; sleep 0.5; pgrep -x Hyprland && pkill -x Hyprland"),
        ["sleeps a fixed time beside the signal"]
    );
    let waited = "stop_shell() { local n='qs|impulse-shell'; pkill -x \"$n\" || return 0\n  \
                  timeout 10 pidwait -x \"$n\"; [ $? != 124 ] || pkill -9 -x \"$n\"; }\n\
                  pkill hyprlock; stop_shell\nkill -TERM $p; timeout 10 pidwait -F $RUN/pid\n\
                  pkill -x kitty; sleep 2";
    assert_eq!(whys(waited), [""; 0]);
}

/// Lines of a script that take a process id from `pgrep` and go on without looking at it: with
/// no such process the id is empty, and `/proc/$P/stat` and `$((b-a))` then fail one by one
/// while the script runs to its end and prints a result.
fn unchecked_pids(text: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim_start().starts_with('#'))
        .filter_map(|(i, l)| {
            let (before, after) = l.split_once("=$(pgrep ")?;
            let var = before
                .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()?;
            let check = format!("[ -n \"${var}\" ] || {{");
            let checked = after
                .split_once(&check)
                .is_some_and(|(_, then)| then.contains("exit 1"));
            (!checked).then_some(i)
        })
        .collect()
}

/// A bench script measures one shell process, found by name once the shell has had time to
/// start. When there is none (the shell did not start, or runs under another name) the script
/// says so and fails, as `bench/vflood.sh` does.
#[test]
fn bench_scripts_stop_when_the_shell_they_measure_is_not_there() {
    let mut scripts = Vec::new();
    files(&root().join("bench"), &mut scripts);
    scripts.retain(|f| f.extension().is_some_and(|e| e == "sh"));
    assert!(scripts.len() > 5, "scripts found: {}", scripts.len());
    let found: Vec<String> = scripts
        .iter()
        .flat_map(|f| {
            let text = std::fs::read_to_string(f).unwrap();
            let name = f.strip_prefix(root()).unwrap().display().to_string();
            unchecked_pids(&text)
                .into_iter()
                .map(|i| format!("{name}:{}: goes on with an empty process id", i + 1))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(found.is_empty(), "\n{}", found.join("\n"));

    let unchecked =
        "sleep 40; P=$(pgrep -x quickshell | head -1); [ -z \"$P\" ] && P=$(pgrep -x qs | head -1)";
    assert_eq!(unchecked_pids(unchecked), [0]);
    let checked = "# P=$(pgrep -x qs)\nsleep 40; P=$(pgrep -x \"$S\" | head -1); \
                   [ -n \"$P\" ] || { echo \"no $S process\" >&2; exit 1; }";
    assert_eq!(unchecked_pids(checked), [0usize; 0]);
}

/// `bench/rss-cpu.sh` adds up the memory of the processes in the shell's cgroup, read one by one
/// from a list. A helper that exits in between has no `/proc/PID/status` any more: awk fails on
/// it, and under `set -e` that ended the whole run with status 2 and no message. It counts as 0.
#[test]
fn rss_cpu_counts_a_helper_that_has_exited_as_zero() {
    let text = std::fs::read_to_string(root().join("bench/rss-cpu.sh")).unwrap();
    let line = |start: &str| {
        text.lines()
            .find(|l| l.trim_start().starts_with(start))
            .unwrap_or_else(|| panic!("bench/rss-cpu.sh has no line that starts with {start}"))
    };
    let script = format!(
        "set -euo pipefail\n{}\np={}\n{}\necho \"kb=${{kb:-0}}\"\n",
        line("status_kb() {"),
        // Larger than any process id (proc(5), pid_max: at most 2^22).
        1u32 << 30,
        line("kb=$(status_kb ")
    );
    let out = std::process::Command::new("bash")
        .args(["-c", &script])
        .output()
        .unwrap();
    assert_eq!(
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).as_ref()
        ),
        (Some(0), "kb=0\n"),
        "{script}{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Where production code waits on the clock, compares against it, or names a duration to do
/// so: `docs/clocks.md` lists each one with why a clock is right there and the test that
/// shows it.
const CLOCKS_DOC: &str = "docs/clocks.md";

/// More ways production code is given a time limit, besides [`POLLS`].
const LIMITS: &[&str] = &[
    "call_timeout(",
    "call_until(",
    "timerfd_settime",
    "update_timer(",
    "timeout_recv",
    "timeout_connect",
    "timeout_global",
    "connect_timeout(",
    "SO_RCVTIMEO",
    "SO_SNDTIMEO",
];

/// Words of a constant's name that say it holds a duration or a point in time.
const CLOCK_WORDS: &[&str] = &[
    "MS",
    "SECS",
    "TIMEOUT",
    "RCVTIMEO",
    "DELAY",
    "DEBOUNCE",
    "RETRY",
    "BACKOFF",
    "EVERY",
    "TTL",
    "GRACE",
    "IDLE",
    "KEEPIDLE",
    "DEADLINE",
    "STALL",
    "STALE",
    "INTERVAL",
    "PERIOD",
    "EYEBALLS",
    "GRANULARITY",
    "FRAMERATE",
];

/// The name and the type of the `const` or `static` that `line` declares.
fn constant(line: &str) -> Option<(&str, &str)> {
    let l = line.trim_start();
    let l = match l.strip_prefix("pub(") {
        Some(l) => l.split_once(") ")?.1,
        None => l.strip_prefix("pub ").unwrap_or(l),
    };
    let l = l
        .strip_prefix("const ")
        .or_else(|| l.strip_prefix("static mut "))
        .or_else(|| l.strip_prefix("static "))?;
    // `const fn f(x: u8)` declares no constant: a name is followed by its `:`.
    let (name, ty) = l.split_once(':')?;
    name.bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        .then(|| (name, ty.split('=').next().unwrap()))
}

/// The name of the constant `line` declares, when one of its words is a [`CLOCK_WORDS`].
fn clock_constant(line: &str) -> Option<&str> {
    let (name, _) = constant(line).filter(|_| !line.contains("static "))?;
    name.split('_')
        .any(|w| CLOCK_WORDS.contains(&w))
        .then_some(name)
}

/// Where `word` stands in `text` as a whole word, not as the end of a longer name.
fn words<'a>(text: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(word).map(|(at, _)| at).filter(|&at| {
        !text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

/// A length of time, whatever it is named: a `const` or `static` whose type holds a
/// `Duration`, or a `Duration` built from a number (`Duration::from_secs(45)`,
/// `Duration::new(s, ns)`, `Duration::try_from_secs_f64(t)`). `Duration::ZERO` is "do not
/// wait", and a parameter or a field of the type only passes one on.
fn duration(line: &str) -> bool {
    const TYPE: &str = "Duration";
    let whole = |text: &str, at: usize| {
        !text[at + TYPE.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
    };
    constant(line).is_some_and(|(_, ty)| words(ty, TYPE).any(|at| whole(ty, at)))
        || words(line, "Duration::").any(|at| {
            let made = &line[at + "Duration::".len()..];
            let name = made.split(|c: char| !c.is_ascii_alphanumeric() && c != '_');
            let name = name.into_iter().next().unwrap();
            (name == "new" || name.starts_with("from_") || name.starts_with("try_from_"))
                && made[name.len()..].starts_with('(')
        })
}

/// A `poll`/`epoll_wait` call on `line` whose last argument, the timeout, is not written as
/// "forever" (`-1`) or "do not wait" (`0`).
fn timed_poll(line: &str) -> bool {
    ["libc::poll(", "epoll_wait("].iter().any(|call| {
        line.find(call).is_some_and(|at| {
            let args = &line[at + call.len()..];
            let (mut depth, mut last) = (0, 0);
            for (i, c) in args.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' if depth == 0 => {
                        let timeout = args[last..i].trim();
                        return last > 0 && timeout != "-1" && timeout != "0";
                    }
                    ')' => depth -= 1,
                    ',' if depth == 0 => last = i + 1,
                    _ => {}
                }
            }
            false
        })
    })
}

/// Elapsed time compared with a limit: `t.elapsed().as_millis() > 5`.
fn elapsed_compared(line: &str) -> bool {
    line.contains(".elapsed().as_")
        && [" < ", " > ", " <= ", " >= "]
            .iter()
            .any(|c| line.contains(c))
}

/// A span of time between two instants compared with a limit:
/// `at.duration_since(l) < LIMIT`. A `Duration` written in the comparison is [`duration`]'s.
fn span_compared(line: &str) -> bool {
    line.contains("duration_since(")
        && [" < ", " > ", " <= ", " >= "]
            .iter()
            .any(|c| line.contains(c))
}

fn clocked(line: &str) -> bool {
    (POLLS
        .iter()
        .any(|p| *p != ".try_wait()" && line.contains(p)))
        || LIMITS.iter().any(|p| line.contains(p))
        || clock_constant(line).is_some()
        || duration(line)
        || timed_poll(line)
        || elapsed_compared(line)
        || span_compared(line)
}

/// The production lines under `crates/*/src` (not the test modules, the tools under
/// `src/bin` or the test support crate) that [`clocked`] finds: `(file, line number, code)`.
fn production_clocks() -> Vec<(String, usize, String)> {
    let root = root();
    let mut units = Vec::new();
    let mut cs: Vec<PathBuf> = std::fs::read_dir(root.join("crates"))
        .unwrap()
        .map(|c| c.unwrap().path())
        .collect();
    cs.sort();
    for c in cs {
        if c.join("src").is_dir() && !WHOLE.iter().any(|w| root.join(w) == c.join("src")) {
            files(&c.join("src"), &mut units);
        }
    }
    units.retain(|f| {
        f.extension().is_some_and(|e| e == "rs") && !f.components().any(|c| c.as_os_str() == "bin")
    });
    let srcs: Vec<(PathBuf, String)> = units
        .into_iter()
        .map(|f| {
            let src = std::fs::read_to_string(&f).unwrap();
            (f, src)
        })
        .collect();
    let mut tests = std::collections::HashSet::new();
    let mut inline = Vec::new();
    for (f, src) in &srcs {
        let (i, o) = test_modules(f, src);
        tests.extend(o);
        inline.push(i);
    }
    let mut found = Vec::new();
    for ((f, src), inline) in srcs.iter().zip(inline) {
        if tests.contains(f) {
            continue;
        }
        let name = f.strip_prefix(&root).unwrap().display().to_string();
        for (k, line) in src.lines().enumerate() {
            let line = code(line).trim();
            if !inline.iter().any(|r| r.contains(&k)) && clocked(line) {
                found.push((name.clone(), k + 1, line.to_owned()));
            }
        }
    }
    found
}

/// A row of `docs/clocks.md`: the file, a piece of the line, and the test (`file` `fn`) that
/// shows the clock at work; no test for a row of the "not waits" table.
struct Row {
    file: String,
    has: String,
    test: Option<(String, String)>,
}

fn rows(doc: &str) -> Vec<Row> {
    let ticked = |cell: &str| -> Vec<String> {
        cell.split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    };
    doc.lines()
        .filter(|l| l.starts_with("| `crates/"))
        .map(|l| {
            let cells: Vec<&str> = l.trim_matches('|').split(" | ").collect();
            assert!(cells.len() >= 3, "{CLOCKS_DOC}: short row: {l}");
            let one = |cell: &str| {
                let t = ticked(cell);
                assert_eq!(t.len(), 1, "{CLOCKS_DOC}: one `code` per cell: {l}");
                t.into_iter().next().unwrap()
            };
            let test = cells.get(3).map(|c| {
                let t = ticked(c);
                assert_eq!(t.len(), 2, "{CLOCKS_DOC}: test is `file` `fn`: {l}");
                (t[0].clone(), t[1].clone())
            });
            Row {
                file: one(cells[0]),
                has: one(cells[1]),
                test,
            }
        })
        .collect()
}

#[test]
fn every_production_clock_is_listed_with_its_reason_and_test() {
    let root = root();
    let doc = std::fs::read_to_string(root.join(CLOCKS_DOC)).unwrap();
    let rows = rows(&doc);
    let found = production_clocks();
    let unlisted: Vec<String> = found
        .iter()
        .filter(|(f, _, l)| !rows.iter().any(|r| r.file == *f && l.contains(&r.has)))
        .map(|(f, n, l)| format!("{f}:{n}: {l}"))
        .collect();
    assert!(
        unlisted.is_empty(),
        "clocks in production code that {CLOCKS_DOC} does not list (wait on the event \
         instead, or list the line with why a clock is right and the test that shows it):\n{}",
        unlisted.join("\n")
    );
    let stale: Vec<String> = rows
        .iter()
        .filter(|r| {
            !found
                .iter()
                .any(|(f, _, l)| r.file == *f && l.contains(&r.has))
        })
        .map(|r| format!("{}: `{}`", r.file, r.has))
        .collect();
    assert!(
        stale.is_empty(),
        "{CLOCKS_DOC} lists clocks the code no longer has:\n{}",
        stale.join("\n")
    );
    let untested: Vec<String> = rows
        .iter()
        .filter_map(|r| r.test.as_ref())
        .filter(|(file, name)| {
            !std::fs::read_to_string(root.join(file))
                .is_ok_and(|src| src.contains(&format!("fn {name}()")))
        })
        .map(|(file, name)| format!("{file}: {name}"))
        .collect();
    assert!(
        untested.is_empty(),
        "{CLOCKS_DOC} names tests that do not exist:\n{}",
        untested.join("\n")
    );
    assert!(
        found.len() > 40,
        "found only {} clocks: is the tree there?",
        found.len()
    );
}

#[test]
fn the_scan_sees_each_way_production_code_uses_the_clock() {
    for l in [
        "pub const GRACE: Duration = Duration::from_secs(2);",
        "const DEBOUNCE_MS: f64 = 100.0;",
        "let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };",
        "bus::call_timeout(",
        "crate::dbus::call_until(",
        "if unsafe { libc::timerfd_settime(fd.as_raw_fd(), 0, &spec, std::ptr::null_mut()) } < 0 {",
        "g = p.cv.wait_timeout(g, at - now).unwrap().0;",
        "&& t0.elapsed().as_millis() > 5",
        "w.reap_last.is_none_or(|t| t.elapsed() >= REAP_EVERY);",
        "at.saturating_duration_since(l) < std::time::Duration::from_micros(100)",
        "if s.stack.is_empty() && now - last > Duration::from_secs(5) {",
        "if now.duration_since(t0).as_secs_f32() >= 1.0 {",
    ] {
        assert!(clocked(l), "{l}");
    }
    for l in [
        "pub const PAM_ERROR_MSG: c_int = 3;",
        "(unsafe { libc::poll(&mut p, 1, 0) }) == 1",
        "let us = t.elapsed().as_micros();",
        "let n = unsafe { libc::poll(fds.as_mut_ptr(), 1, -1) };",
        "let n = unsafe { libc::poll(&mut p, 1, 0) };",
        "fn poll(w: &mut World, calls: Vec<crate::svc::Call>) -> bool {",
        "let status = child.try_wait();",
    ] {
        assert!(!clocked(l), "{l}");
    }
}

#[test]
fn a_length_of_time_is_a_clock_whatever_it_is_named() {
    for l in [
        "const TRY_LOCK: Duration = Duration::from_secs(45);",
        "pub(crate) const LIMIT: std::time::Duration =",
        "pub(in crate::ports) static LIMIT: Duration = LONG;",
        "static CROSSFADE: std::sync::Mutex<(u64, std::time::Duration)> =",
        "static mut LAST: Option<Duration> = None;",
        "Lock::take_within(path, Duration::from_secs(2))",
        "answer_within: Some(std::time::Duration::from_secs(2)),",
        "let d = std::time::Duration::from_secs_f64(uniform(wait * 0.9, wait * 1.1));",
        "(rc == 0).then(|| Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32))",
        "start.checked_add(Duration::try_from_secs_f64(secs).ok()?)",
        "wait(core::time::Duration::from_millis(ms));",
    ] {
        assert!(clocked(l), "{l}");
    }
    for l in [
        "use std::time::Duration;",
        "pub fn take_within(path: &Path, limit: Duration) -> io::Result<Lock> {",
        "const fn limit(of: Duration) -> Duration {",
        "answer_within: Option<Duration>,",
        "let mut delay = std::time::Duration::ZERO;",
        "const DURATIONS: usize = 3;",
        "const EASING: AnimDuration = AnimDuration::new(3);",
        "static NAMES: [&str; 2] = [\"a\", \"b\"];",
        "let d = TweenDuration::from_secs(1) + Durations::new(2);",
        "let took = t.elapsed().as_secs_f64();",
    ] {
        assert!(!clocked(l), "{l}");
    }
}
