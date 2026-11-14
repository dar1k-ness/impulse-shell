//! Equivalence of `colorgen::kde` with kde-material-you-colors 1.10.1: `tools/reference/kmyc-golden`
//! ran the original once per case in a sandbox HOME under `dbus-monitor`; this replays every case
//! natively and compares what Qt/KDE apps get to see. Plasma schemes and KSyntaxHighlighting
//! themes must be byte-identical; KConfig files (kdeglobals, Konsole) must hold the same entries;
//! the bus must carry the same notifications. Deliberate differences, checked as such:
//! no `*2.colors` / `*_darker_titlebar*` copies, no `/tmp` JSON dump, comments kept, Klassy /
//! SierraBreeze titlebars left alone unless `titlebar_opacity` is set, and no KWin calls.
//! pywal (`pywal = True`): `~/.cache/wal` byte-identical, the terminals get its `sequences`.

use colorgen::kde::kconfig::Doc;
use colorgen::kde::{self, Body, Bus, Dirs, Outcome};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/kde");

#[derive(Default)]
struct Rec(
    Mutex<Vec<(String, Body)>>,
    Mutex<Vec<Vec<u8>>>,
    Mutex<usize>,
);

const FAKE_TTY: &str = "/dev/pts/kde-test";

impl Bus for Rec {
    fn signal(&self, path: &str, iface: &str, member: &str, body: Body) {
        self.0
            .lock()
            .unwrap()
            .push((format!("{path} {iface}.{member}"), body));
    }
    fn has_owner(&self, _: &str) -> bool {
        false
    }
    fn call(&self, dest: &str, path: &str, iface: &str, member: &str) {
        panic!("unexpected call {dest} {path} {iface}.{member}");
    }
    fn konsole_sessions(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    fn konsole_profile(&self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn set_konsole_profile(&self, _: &str, _: &str, _: &str) {}
    fn ttys(&self) -> Vec<PathBuf> {
        vec![PathBuf::from(FAKE_TTY)]
    }
    fn write_tty(&self, tty: &Path, data: &[u8]) {
        assert_eq!(tty, Path::new(FAKE_TTY));
        self.1.lock().unwrap().push(data.to_vec());
    }
    fn pywal_reload(&self, _: &Path) {
        *self.2.lock().unwrap() += 1;
    }
}

fn files(root: &Path) -> BTreeSet<String> {
    fn walk(root: &Path, d: &Path, out: &mut BTreeSet<String>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.insert(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(root, root, &mut out);
    out
}

fn copy_tree(from: &Path, to: &Path) {
    for f in files(from) {
        let dst = to.join(&f);
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::copy(from.join(&f), dst).unwrap();
    }
}

/// The expected file `rel` of `case`, through `shared.txt`.
fn expected(case: &str, rel: &str) -> Option<Vec<u8>> {
    let base = Path::new(DIR).join("expected");
    let owner = std::fs::read_to_string(base.join(case).join("shared.txt"))
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            l.split_once('\t')
                .filter(|(r, _)| *r == rel)
                .map(|(_, o)| o.to_string())
        })
        .unwrap_or_else(|| case.to_string());
    std::fs::read(base.join(owner).join(rel)).ok()
}

fn expected_files(case: &str) -> BTreeSet<String> {
    let base = Path::new(DIR).join("expected").join(case);
    let mut s = files(&base);
    for l in std::fs::read_to_string(base.join("shared.txt"))
        .unwrap_or_default()
        .lines()
    {
        if let Some((r, _)) = l.split_once('\t') {
            s.insert(r.to_string());
        }
    }
    s.remove("shared.txt");
    s
}

type Entries = BTreeMap<String, BTreeMap<String, String>>;

fn entries(text: &str) -> Entries {
    let d = Doc::parse(text);
    d.groups()
        .into_iter()
        .map(|g| {
            let e: BTreeMap<String, String> = d
                .entries(g)
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect();
            (g.to_string(), e)
        })
        .filter(|(_, e)| !e.is_empty())
        .collect()
}

/// Signals in a dbus-monitor log: (path iface.member, ints) and the ConfigChanged keys.
fn bus_log(log: &str) -> (BTreeMap<String, Vec<Vec<i32>>>, BTreeSet<String>) {
    let mut sigs: BTreeMap<String, Vec<Vec<i32>>> = BTreeMap::new();
    let mut keys = BTreeSet::new();
    let mut cur: Option<String> = None;
    let mut group = String::new();
    for l in log.lines() {
        let t = l.trim();
        if let Some(rest) = l.strip_prefix("signal ") {
            let f = |k: &str| {
                rest.split(&format!("{k}="))
                    .nth(1)
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
                    .to_string()
            };
            let name = format!("{} {}.{}", f("path"), f("interface"), f("member"));
            sigs.entry(name.clone()).or_default().push(Vec::new());
            cur = Some(name);
        } else if !l.starts_with(' ') {
            cur = None;
        } else if let Some(name) = &cur {
            if let Some(i) = t.strip_prefix("int32 ") {
                sigs.get_mut(name)
                    .unwrap()
                    .last_mut()
                    .unwrap()
                    .push(i.parse().unwrap());
            } else if let Some(g) = t.strip_prefix("string \"") {
                group = g.trim_end_matches('"').to_string();
            } else if let Some(k) = t.strip_prefix("array of bytes \"") {
                keys.insert(format!("{group}/{}", k.trim_end_matches('"')));
            }
        }
    }
    (sigs, keys)
}

const SKIPPED: [&str; 2] = ["2.colors", "_darker_titlebar"];
const OPACITY: [(&str, &str); 3] = [
    ("TitleBarOpacity", "ActiveTitleBarOpacity"),
    ("TitleBarOpacity", "InactiveTitleBarOpacity"),
    ("Windeco", "BackgroundOpacity"),
];

fn run_case(case: &str) -> Result<(), String> {
    let cdir = Path::new(DIR).join("cases").join(case);
    let args: Vec<String> = std::fs::read_to_string(cdir.join("args"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    let dark = args.iter().any(|a| a == "-d");
    let at = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .map(|i| args[i + 1].clone())
    };
    let seed = at("--color").unwrap();
    let sv: usize = at("-sv").map_or(5, |v| v.parse().unwrap());
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().to_path_buf();
    if cdir.join("home").is_dir() {
        copy_tree(&cdir.join("home"), &home);
    }
    if cdir.join("config.conf").is_file() {
        let d = home.join(".config/kde-material-you-colors");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::copy(cdir.join("config.conf"), d.join("config.conf")).unwrap();
    }
    let before = files(&home);
    let config = std::fs::read_to_string(cdir.join("config.conf")).unwrap_or_default();
    let dirs = Dirs {
        config: home.join(".config"),
        data: home.join(".local/share"),
        cache: home.join(".cache"),
    };
    let bus = Rec::default();
    match kde::recolor(&dirs, &seed, dark, sv, &bus) {
        Outcome::Applied(_) => {}
        o => return Err(format!("{o:?}")),
    }
    let mut errs = Vec::new();
    let ours = files(&home);
    for rel in expected_files(case) {
        if matches!(
            rel.as_str(),
            "export.json" | "dbus.log" | "outcome.txt" | "deleted.txt"
        ) {
            continue;
        }
        let exp = expected(case, &rel).unwrap();
        let path = home.join(&rel);
        let got = std::fs::read(&path).unwrap_or_default();
        let diff = |g: &Entries, e: &Entries, errs: &mut Vec<String>| {
            for k in g.keys().chain(e.keys()).collect::<BTreeSet<_>>() {
                if g.get(k) != e.get(k) {
                    errs.push(format!(
                        "{rel} [{k}]: ours {:?} original {:?}",
                        g.get(k),
                        e.get(k)
                    ));
                }
            }
        };
        let text = |b: &[u8]| entries(&String::from_utf8_lossy(b));
        if SKIPPED.iter().any(|s| rel.contains(s)) {
            // Only the darker-titlebar scheme a KWin rule points at is written.
            if !before.contains(&rel) && path.exists() {
                if rel.contains("_darker_titlebar") {
                    diff(&text(&got), &text(&exp), &mut errs);
                } else {
                    errs.push(format!("{rel}: written, should be skipped"));
                }
            }
            continue;
        }
        let titlebar = rel.ends_with("klassyrc") || rel.ends_with("sierrabreezeenhancedrc");
        if titlebar {
            // The original resets titlebar opacity even when titlebar_opacity is unset.
            let orig = text(&std::fs::read(cdir.join("home").join(&rel)).unwrap_or_default());
            let mut e = text(&exp);
            if !config.contains("titlebar_opacity") {
                for (g, k) in OPACITY {
                    let o = orig.get(g).and_then(|m| m.get(k));
                    match (o, e.get_mut(g)) {
                        (Some(v), Some(m)) => {
                            m.insert(k.into(), v.clone());
                        }
                        (None, Some(m)) => {
                            m.remove(k);
                        }
                        _ => {}
                    }
                }
                e.retain(|_, m| !m.is_empty());
            }
            if rel.ends_with("klassyrc") {
                // The original reuses one configparser for both files: SierraBreezeEnhanced
                // entries leak into klassyrc.
                let sbe = ".config/sierrabreezeenhancedrc";
                let sbe = text(
                    &expected(case, sbe)
                        .or_else(|| std::fs::read(cdir.join("home").join(sbe)).ok())
                        .unwrap_or_default(),
                );
                for (g, m) in &sbe {
                    if let Some(em) = e.get_mut(g) {
                        em.retain(|k, _| {
                            !m.contains_key(k) || orig.get(g).is_some_and(|o| o.contains_key(k))
                        });
                    }
                }
                e.retain(|_, m| !m.is_empty());
            }
            diff(&text(&got), &e, &mut errs);
        } else if rel.ends_with(".colors")
            || rel.ends_with(".theme")
            || rel.starts_with(".cache/wal/")
        {
            if got != exp {
                errs.push(format!("{rel}: differs"));
            }
        } else {
            let (g, e) = (
                entries(&String::from_utf8_lossy(&got)),
                entries(&String::from_utf8_lossy(&exp)),
            );
            if g != e {
                for k in g.keys().chain(e.keys()).collect::<BTreeSet<_>>() {
                    if g.get(k) != e.get(k) {
                        errs.push(format!(
                            "{rel} [{k}]: ours {:?} original {:?}",
                            g.get(k),
                            e.get(k)
                        ));
                    }
                }
            }
        }
    }
    for rel in ours.difference(&expected_files(case)) {
        if !before.contains(rel) {
            errs.push(format!("{rel}: extra file"));
        } else if std::fs::read(home.join(rel)).ok()
            != std::fs::read(cdir.join("home").join(rel)).ok()
            && !rel.ends_with("kde-material-you-colors/config.conf")
        {
            errs.push(format!("{rel}: changed, the original left it as it was"));
        }
    }
    // pywal: the terminals got what `~/.cache/wal/sequences` holds, then one reload.
    let seq = expected(case, ".cache/wal/sequences");
    let ttys = bus.1.lock().unwrap().clone();
    match &seq {
        Some(s) if ttys != [s.clone()] => errs.push(format!("terminals got {ttys:?}")),
        None if !ttys.is_empty() => errs.push("terminals written without pywal".into()),
        _ => {}
    }
    let reloads = *bus.2.lock().unwrap();
    if reloads != usize::from(seq.is_some()) {
        errs.push(format!("{reloads} pywal reloads"));
    }
    let (exp_sigs, exp_keys) =
        bus_log(&String::from_utf8(expected(case, "dbus.log").unwrap()).unwrap());
    let mut got_keys = BTreeSet::new();
    let mut got_sigs: BTreeMap<String, Vec<Vec<i32>>> = BTreeMap::new();
    for (name, body) in bus.0.into_inner().unwrap() {
        let ints = match body {
            Body::Ints(v) => v,
            Body::Changed(g) => {
                for (g, ks) in g {
                    got_keys.extend(ks.into_iter().map(|k| format!("{g}/{k}")));
                }
                Vec::new()
            }
            Body::Empty => Vec::new(),
        };
        got_sigs.entry(name).or_default().push(ints);
    }
    let dedup = |m: BTreeMap<String, Vec<Vec<i32>>>| -> BTreeMap<String, BTreeSet<Vec<i32>>> {
        m.into_iter()
            .filter(|(k, _)| !k.contains("Klassy") && !k.contains("ConfigChanged"))
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect()
    };
    if dedup(got_sigs.clone()) != dedup(exp_sigs) {
        errs.push(format!("signals: ours {got_sigs:?}"));
    }
    // The original re-applied through `MaterialYouDark2` first, so it always flipped the name;
    // an unchanged name is not announced here.
    let name = if dark {
        "MaterialYouDark"
    } else {
        "MaterialYouLight"
    };
    let kg_before =
        std::fs::read_to_string(cdir.join("home/.config/kdeglobals")).unwrap_or_default();
    let mut exp_keys = exp_keys;
    if Doc::parse(&kg_before).get("General", "ColorScheme") == Some(name) {
        exp_keys.remove("General/ColorScheme");
    }
    if got_keys != exp_keys {
        errs.push(format!(
            "ConfigChanged keys: ours {got_keys:?} original {exp_keys:?}"
        ));
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs.join("\n  "))
    }
}

#[test]
fn kde_material_you_colors_cases() {
    let mut cases: Vec<PathBuf> = std::fs::read_dir(Path::new(DIR).join("cases"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    cases.sort();
    let fails: Vec<String> = cases
        .iter()
        .map(|c| c.file_name().unwrap().to_string_lossy().into_owned())
        .filter_map(|c| run_case(&c).err().map(|e| format!("{c}:\n  {e}")))
        .collect();
    assert!(
        fails.is_empty(),
        "{} of {} cases differ:\n{}",
        fails.len(),
        cases.len(),
        fails.join("\n")
    );
}

/// `applyScheme` on accent settings kde-material-you-colors never writes into its schemes
/// (`TintFactor`, accented titlebars) and on Breeze: `tools/reference/kmyc-golden/plasma.sh` ran the real
/// `plasma-apply-colorscheme` on each `plasma/cases/<case>` (kdeglobals with an `AccentColor`).
#[test]
fn plasma_apply_colorscheme_cases() {
    let base = Path::new(DIR).join("plasma");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(base.join("cases"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    cases.sort();
    assert!(!cases.is_empty());
    let mut failed = Vec::new();
    for dir in cases {
        let case = dir.file_name().unwrap().to_string_lossy().into_owned();
        let mut kg = Doc::parse(&std::fs::read_to_string(dir.join("kdeglobals")).unwrap());
        kde::apply::apply_scheme_text(
            &mut kg,
            &std::fs::read_to_string(dir.join("Test.colors")).unwrap(),
            "Test",
        );
        let exp =
            std::fs::read_to_string(base.join("expected").join(&case).join("kdeglobals")).unwrap();
        let (g, e) = (
            entries(&String::from_utf8(kg.bytes()).unwrap()),
            entries(&exp),
        );
        for k in g.keys().chain(e.keys()).collect::<BTreeSet<_>>() {
            if g.get(k) != e.get(k) {
                failed.push(format!(
                    "{case} [{k}]: ours {:?} plasma {:?}",
                    g.get(k),
                    e.get(k)
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
