//! Native `Rustshell.DesktopEntries` (quickshell src/core/desktopentry.cpp): XDG application
//! scan, `applications` (NoDisplay filtered), `byId`, `heuristicLookup`, `execute`.
use crate::runtime::World;
use crate::value::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct State {
    by_id: HashMap<String, usize>,
    lower: HashMap<String, usize>,
    /// (item, StartupWMClass) in id order.
    startup: Vec<(usize, String)>,
    /// Rescan time on the loop's clock (`World::now`, ms): DesktopEntryMonitor's 100 ms
    /// single-shot debounce, restarted by each change.
    due: Option<f64>,
    /// Watches armed and the first scan started.
    loaded: bool,
    /// Scans run on a pool worker and report (scan number, entries) here.
    scans: Option<(
        crate::svc::Sender<Scanned>,
        std::sync::mpsc::Receiver<Scanned>,
    )>,
    /// Scans started / the newest one applied (0: none yet).
    started: u64,
    applied: u64,
}

/// A finished scan: its number and the entries found.
type Scanned = (u64, Vec<Data>);

#[derive(Default, Clone)]
struct Action {
    id: String,
    name: String,
    icon: String,
    exec: String,
}

#[derive(Default)]
struct Data {
    id: String,
    name: String,
    generic: String,
    startup: String,
    comment: String,
    icon: String,
    exec: String,
    path: String,
    no_display: bool,
    hidden: bool,
    terminal: bool,
    categories: Vec<String>,
    keywords: Vec<String>,
    actions: Vec<Action>,
}

/// The environment is fixed for the process.
fn paths() -> &'static [PathBuf] {
    static PATHS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    PATHS.get_or_init(app_dirs)
}

fn app_dirs() -> Vec<PathBuf> {
    let mut v = vec![];
    {
        v.push(crate::xdg::data_home().join("applications"));
    }
    for d in crate::icons::xdg_data_dirs() {
        v.push(PathBuf::from(d).join("applications"));
    }
    v
}

fn locales() -> (String, String) {
    let l = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|s| !s.is_empty()))
        .unwrap_or_default();
    let full = l.split(['.', '@']).next().unwrap_or("").to_string();
    let lang = full.split('_').next().unwrap_or("").to_string();
    (full, lang)
}

/// `QString::split(';', SkipEmptyParts)`
fn split_list(s: &str) -> Vec<String> {
    s.split(';')
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

/// DesktopEntry::parseExecString
pub fn parse_exec(s: &str) -> Vec<String> {
    let mut args = vec![];
    let mut cur = String::new();
    let (mut string, mut escape, mut percent) = (false, 0, false);
    for c in s.chars() {
        if escape == 0 && c == '\\' {
            escape = 1;
        } else if string {
            if c == '\\' {
                escape += 1;
                if escape == 4 {
                    cur.push('\\');
                    escape = 0;
                }
            } else if escape == 2 {
                cur.push(c);
                escape = 0;
            } else if escape != 0 {
                cur.push(match c {
                    's' => ' ',
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    c => c,
                });
                escape = 0;
            } else if c == '"' || c == '\'' {
                string = false;
            } else {
                cur.push(c);
            }
        } else if escape != 0 {
            cur.push(c);
            escape = 0;
        } else if percent {
            if c == '%' {
                cur.push('%');
            }
            percent = false;
        } else if c == '%' {
            percent = true;
        } else if c == '"' || c == '\'' {
            string = true;
        } else if c == ' ' {
            if !cur.is_empty() {
                args.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        args.push(cur);
    }
    args
}

/// A `[group]` of a desktop entry and its keys.
type Group = (String, HashMap<String, (u8, String)>);

fn parse(id: &str, text: &str, loc: &(String, String)) -> Option<Data> {
    let mut d = Data {
        id: id.to_string(),
        ..Default::default()
    };
    let mut groups: Vec<Group> = vec![];
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            groups.push((line[1..line.len() - 1].to_string(), HashMap::new()));
            continue;
        }
        let (Some((k, v)), Some(g)) = (line.split_once('='), groups.last_mut()) else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        let (key, score) = match k.split_once('[') {
            Some((base, l)) => {
                let l = l.trim_end_matches(']');
                let l = l.split(['.', '@']).next().unwrap_or("");
                if !loc.0.is_empty() && l == loc.0 {
                    (base, 2)
                } else if !loc.1.is_empty() && l == loc.1 {
                    (base, 1)
                } else {
                    continue;
                }
            }
            None => (k, 0),
        };
        if g.1.get(key).is_none_or(|(s, _)| score >= *s) {
            g.1.insert(key.to_string(), (score, v.to_string()));
        }
    }
    let mut order = vec![];
    let mut actions: HashMap<String, Action> = HashMap::new();
    let mut is_app = false;
    for (name, e) in &groups {
        let get = |k: &str| e.get(k).map(|p| p.1.clone()).unwrap_or_default();
        if name == "Desktop Entry" {
            if get("Type") != "Application" {
                continue;
            }
            is_app = true;
            d.name = get("Name");
            d.generic = get("GenericName");
            d.startup = get("StartupWMClass");
            d.no_display = get("NoDisplay") == "true";
            d.hidden = get("Hidden") == "true";
            d.comment = get("Comment");
            d.icon = get("Icon");
            d.exec = get("Exec");
            d.path = get("Path");
            d.terminal = get("Terminal") == "true";
            d.categories = split_list(&get("Categories"));
            d.keywords = split_list(&get("Keywords"));
            order = split_list(&get("Actions"));
        } else if let Some(a) = name.strip_prefix("Desktop Action ") {
            actions.insert(
                a.to_string(),
                Action {
                    id: a.to_string(),
                    name: get("Name"),
                    icon: get("Icon"),
                    exec: get("Exec"),
                },
            );
        }
    }
    d.actions = order
        .iter()
        .filter_map(|a| actions.get(a).cloned())
        .collect();
    (is_app || d.hidden).then_some(d)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut es: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    es.sort();
    for p in es {
        if p.is_dir() {
            walk(root, &p, out);
        } else if p.extension().is_some_and(|e| e == "desktop")
            && let Ok(rel) = p.strip_prefix(root)
        {
            // Only the extension: `org.telegram.desktop.desktop` is `org.telegram.desktop`.
            let rel = rel.to_string_lossy();
            let id = rel
                .strip_suffix(".desktop")
                .unwrap_or(&rel)
                .replace('/', "-");
            out.push((id, p));
        }
    }
}

/// Highest-priority directory wins; `Hidden=true` masks lower ones.
fn scan(ps: &[PathBuf]) -> Vec<Data> {
    let loc = locales();
    let mut map: HashMap<String, Data> = HashMap::new();
    for root in ps.iter().rev() {
        let mut files = vec![];
        walk(root, root, &mut files);
        for (id, p) in files {
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            match parse(&id, &text, &loc) {
                Some(d) if d.hidden => {
                    map.remove(&id);
                }
                Some(d) if !d.name.is_empty() => {
                    map.insert(id, d);
                }
                _ => {}
            }
        }
    }
    let mut v: Vec<Data> = map.into_values().collect();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

fn strs(v: &[String]) -> Value {
    Value::array(v.iter().map(|s| Value::str(s)).collect())
}

fn fill(w: &mut World, i: usize, d: &Data) {
    w.write(i, "id", Value::str(&d.id));
    w.write(i, "name", Value::str(&d.name));
    w.write(i, "genericName", Value::str(&d.generic));
    w.write(i, "startupClass", Value::str(&d.startup));
    w.write(i, "noDisplay", Value::Bool(d.no_display));
    w.write(i, "comment", Value::str(&d.comment));
    w.write(i, "icon", Value::str(&d.icon));
    w.write(i, "execString", Value::str(&d.exec));
    w.write(i, "command", strs(&parse_exec(&d.exec)));
    w.write(i, "workingDirectory", Value::str(&d.path));
    w.write(i, "runInTerminal", Value::Bool(d.terminal));
    w.write(i, "categories", strs(&d.categories));
    w.write(i, "keywords", strs(&d.keywords));
    let acts = d
        .actions
        .iter()
        .map(|a| {
            let ai = crate::types::native_singleton(w, "DesktopAction");
            w.write(ai, "id", Value::str(&a.id));
            w.write(ai, "name", Value::str(&a.name));
            w.write(ai, "icon", Value::str(&a.icon));
            w.write(ai, "execString", Value::str(&a.exec));
            w.write(ai, "command", strs(&parse_exec(&a.exec)));
            Value::Item(ai)
        })
        .collect();
    w.write(i, "actions", Value::array(acts));
}

fn load(w: &mut World, st: &mut State, de: usize, datas: Vec<Data>) {
    let mut by_id = HashMap::new();
    let mut lower = HashMap::new();
    let mut startup = vec![];
    let mut apps = vec![];
    for d in &datas {
        let i = match st.by_id.remove(&d.id) {
            Some(i) => i,
            None => crate::types::native_singleton(w, "DesktopEntry"),
        };
        fill(w, i, d);
        by_id.insert(d.id.clone(), i);
        let l = d.id.to_lowercase();
        if lower.insert(l.clone(), i).is_some() {
            lower.remove(&l);
        }
        startup.push((i, d.startup.clone()));
        if !d.no_display {
            apps.push(Value::Item(i));
        }
    }
    for (_, gone) in st.by_id.drain() {
        w.destroy(gone);
    }
    st.by_id = by_id;
    st.lower = lower;
    st.startup = startup;
    if let Some(Value::Item(m)) = w.read(de, "applications") {
        w.write(m, "values", Value::array(apps));
    }
}

thread_local! {
    static INLINE_SCANS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Scan on this thread instead of a pool worker, so a scan lands at the poll that started it
/// whatever the machine load (a harness comparing two runtimes step by step).
pub fn set_inline_scans(on: bool) {
    INLINE_SCANS.with(|c| c.set(on));
}

/// Scan the application dirs on a pool worker; the main loop applies the result in [`poll`].
fn start_scan(w: &mut World) {
    let tx = w
        .desktop
        .scans
        .get_or_insert_with(|| w.waker.handle(crate::svc::Svc::Fswatch).channel())
        .0
        .clone();
    w.desktop.started += 1;
    let n = w.desktop.started;
    if INLINE_SCANS.with(|c| c.get()) {
        let _ = tx.send((n, scan(paths())));
        return;
    }
    workpool::run(move || {
        let _ = tx.send((n, scan(paths())));
    });
}

/// Apply the newest finished scan.
fn apply_finished(w: &mut World, de: usize) -> bool {
    let Some((_, rx)) = &w.desktop.scans else {
        return false;
    };
    let mut newest: Option<Scanned> = None;
    for r in rx.try_iter() {
        if newest.as_ref().is_none_or(|n| r.0 > n.0) {
            newest = Some(r);
        }
    }
    let Some((n, datas)) = newest.filter(|n| n.0 > w.desktop.applied) else {
        return false;
    };
    apply(w, de, n, datas);
    true
}

fn apply(w: &mut World, de: usize, n: u64, datas: Vec<Data>) {
    let mut st = std::mem::take(&mut w.desktop);
    st.applied = n;
    load(w, &mut st, de, datas);
    w.desktop = st;
}

const DEBOUNCE_MS: f64 = 100.0;

/// Scan on first use (after arming the directory watches, like DesktopEntryManager's
/// constructor), rescan once DEBOUNCE_MS of loop time (`World::now`) has passed since the last
/// directory change. Scans run on a pool worker, never on the main loop: until a rescan lands the previous entries stay
/// (no freeze on a package install), and the very first `applications` is empty until the
/// first scan lands. `byId` / `heuristicLookup` before then scan on the spot ([`ensure`]),
/// so a lookup never misses an installed app.
pub fn poll(w: &mut World) -> bool {
    let Some(&de) = w.singletons.get("DesktopEntries") else {
        return false;
    };
    if !w.desktop.loaded {
        crate::fswatch::watch_desktop_dirs(w, paths());
        w.desktop.loaded = true;
        start_scan(w);
    }
    let mut changed = apply_finished(w, de);
    let now = w.now;
    if crate::fswatch::desktop_dirs_changed(w) {
        w.desktop.due = Some(now + DEBOUNCE_MS);
    }
    if w.desktop.due.is_some_and(|d| now >= d) {
        w.desktop.due = None;
        start_scan(w);
        // A fast scan may already be back.
        changed |= apply_finished(w, de);
    }
    changed
}

/// Milliseconds from `now` until a pending rescan is due.
pub(crate) fn rescan_in_ms(w: &World, now: f64) -> Option<f64> {
    w.desktop.due.map(|d| (d - now).max(0.0))
}

fn ensure(w: &mut World) {
    // Native callers (a notification's `desktop-entry`) may come before any QML has named
    // `DesktopEntries`; Quickshell's DesktopEntryManager exists regardless.
    if !w.singletons.contains_key("DesktopEntries") {
        w.type_value("DesktopEntries");
    }
    if !w.desktop.loaded {
        poll(w);
    }
    // A lookup before the first scan landed scans here, as Quickshell's synchronous scan
    // would have, instead of waiting for the pool's scan: that one may still queue behind
    // other jobs, and lands later as an older scan than this one, so it is dropped.
    if w.desktop.applied == 0
        && let Some(&de) = w.singletons.get("DesktopEntries")
        && !apply_finished(w, de)
    {
        w.desktop.started += 1;
        let n = w.desktop.started;
        apply(w, de, n, scan(paths()));
    }
}

pub fn by_id(w: &mut World, id: &str) -> Value {
    ensure(w);
    let st = &w.desktop;
    st.by_id
        .get(id)
        .or_else(|| st.lower.get(&id.to_lowercase()))
        .map_or(Value::Null, |&i| Value::Item(i))
}

pub fn heuristic_lookup(w: &mut World, name: &str) -> Value {
    let v = by_id(w, name);
    if !matches!(v, Value::Null) {
        return v;
    }
    let st = &w.desktop;
    let l = name.to_lowercase();
    st.startup
        .iter()
        .find(|(_, s)| s == name)
        .or_else(|| st.startup.iter().find(|(_, s)| s.to_lowercase() == l))
        .map_or(Value::Null, |&(i, _)| Value::Item(i))
}

/// DesktopEntry::execute / DesktopAction::execute (doExec: detached, entry working directory).
pub fn execute(w: &mut World, item: usize) {
    let argv: Vec<String> = match w.read(item, "command") {
        Some(Value::Obj(o)) => o
            .borrow()
            .array
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|v| v.to_js_string())
            .collect(),
        _ => vec![],
    };
    if argv.is_empty() {
        return;
    }
    if crate::native::hooked_spawn(&argv).is_some() {
        return;
    }
    let cwd = w.read(item, "workingDirectory").map(|v| v.to_js_string());
    // Quickshell's DesktopEntry::doExec: execDetached of the command in the entry's directory.
    crate::native::detach(
        "DesktopEntry.execute",
        crate::detached::Launch::inherit(&argv, cwd),
    );
}

#[cfg(test)]
mod tests {
    /// `DesktopEntry.execute()` starts the command as Quickshell's `doExec` does
    /// (`execDetached`): leading a session of its own, in the entry's working directory. It
    /// used to stay in the shell's session and group.
    #[test]
    fn execute_starts_the_command_in_its_own_session() {
        use crate::native::tests::{REPORT, assert_detached, report, report_fifo};
        let (dir, f) = report_fifo();
        let cwd = dir.path().canonicalize().unwrap();
        let mut rt = crate::runtime::Runtime::new(None);
        let entry = rt
            .load_str(&format!(
                "import QtQuick\nQtObject {{ property var command: ['sh', '-c', {REPORT:?}, {f:?}]\n\
                 property string workingDirectory: {cwd:?} }}"
            ))
            .unwrap();
        rt.settle();
        super::execute(&mut rt.world.borrow_mut(), entry);
        let r = report(&f);
        assert_detached(&r);
        assert_eq!(r.cwd, cwd.to_str().unwrap());
        workpool::wait_idle();
    }

    /// The first lookup (a notification's `desktop-entry`) comes while every bounded worker
    /// is busy (Hyprland requests, D-Bus calls), so the scan it started is still queued: it
    /// still finds the installed app, at once.
    #[test]
    fn a_lookup_before_the_first_scan_finds_installed_apps_with_the_workers_busy() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_lookup_before_the_first_scan_finds_installed_apps_with_the_workers_busy"
        );
        if !crate::testenv::in_own_process(TEST) {
            let base = tempfile::tempdir().unwrap();
            let (home, sys) = (base.path().join("home"), base.path().join("sys"));
            std::fs::create_dir_all(home.join("applications")).unwrap();
            std::fs::create_dir_all(sys.join("applications")).unwrap();
            std::fs::write(
                home.join("applications/app.desktop"),
                "[Desktop Entry]\nType=Application\nName=App\nExec=true\n",
            )
            .unwrap();
            crate::testenv::run_in_own_process(
                TEST,
                [("XDG_DATA_HOME", &home), ("XDG_DATA_DIRS", &sys)],
            );
            return;
        }
        let busy: Vec<_> = (0..workpool::WORKERS)
            .map(|_| {
                let (tx, rx) = std::sync::mpsc::channel::<()>();
                workpool::run(move || {
                    let _ = rx.recv();
                });
                tx
            })
            .collect();
        let rt = crate::runtime::Runtime::new(None);
        let mut w = rt.world.borrow_mut();
        assert!(matches!(
            super::by_id(&mut w, "app"),
            crate::value::Value::Item(_)
        ));
        assert_eq!((w.desktop.started, w.desktop.applied), (2, 2));
        drop(busy);
        workpool::wait_idle();
        // The pool's scan, older than the one applied, changes nothing.
        assert!(!super::poll(&mut w));
    }

    #[test]
    fn inline_scan_lands_at_the_poll_that_started_it() {
        super::set_inline_scans(true);
        let rt = crate::runtime::Runtime::new(None);
        let mut w = rt.world.borrow_mut();
        w.type_value("DesktopEntries");
        super::poll(&mut w);
        assert_eq!((w.desktop.started, w.desktop.applied), (1, 1));
        super::set_inline_scans(false);
    }
}
