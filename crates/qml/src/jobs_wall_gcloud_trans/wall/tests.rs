//! random_konachan_wall.sh / random_osu_wall.sh against the Rust job: same stdout (that of
//! switchwall.sh), exit code and files under Wallpapers. The scripts run unmodified; a `curl`
//! wrapper on PATH sends their two hard-coded hosts to a local server. Both sides end in a
//! stub switchwall (the script's `SWITCHWALL`, the job's `stub_switchwall`); `wall_runs_switchwall_in_process`
//! checks that the real job hands that step to the in-process `colorgen::switchwall`.
use super::super::testutil::{self, Resp, Server};
use super::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Fixture {
    dir: PathBuf,
    home: PathBuf,
    random: PathBuf,
    bin: PathBuf,
    srv: Server,
    /// Where both curl and the job are sent instead of `srv` (a dead port).
    base: Option<String>,
}

const SWITCHWALL: &str = r#"#!/usr/bin/env bash
echo "switchwall $*"
if [ -f "$2" ]; then echo "size $(stat -c %s "$2")"; else echo "no file"; fi
echo "switchwall stderr" >&2
exit 3
"#;

/// `SWITCHWALL` for the job's in-process switchwall step.
fn stub_switchwall(_: &Ctx, req: &Request, io: Io) -> i32 {
    assert_eq!(
        *req,
        Request {
            image: req.image.clone(),
            ..Default::default()
        }
    );
    let img = &req.image;
    (io.out)(format!("switchwall --image {img}\n"));
    (io.out)(match std::fs::metadata(img) {
        Ok(m) if m.is_file() => format!("size {}\n", m.len()),
        _ => "no file\n".into(),
    });
    (io.err)("switchwall stderr\n".into());
    3
}

/// Ctx fields the scripts' comparisons do not vary.
fn stub() -> Ctx {
    Ctx {
        home: String::new(),
        xdg_config_home: String::new(),
        xdg_pictures_dir: None,
        has_xdg_user_dir: true,
        script_dir: String::new(),
        konachan: String::new(),
        osu: String::new(),
        switchwall: stub_switchwall,
        sys: Arc::new(Fake::default()),
        xdg_cache_home: String::new(),
        xdg_state_home: String::new(),
    }
}

/// A session that records what switchwall asks of it.
#[derive(Default)]
struct Fake {
    log: std::sync::Mutex<Vec<String>>,
    logged: std::sync::Condvar,
}

impl Fake {
    fn log(&self, s: String) {
        self.log.lock().unwrap().push(s);
        self.logged.notify_all();
    }

    /// The run's log once `notifies` notifications are in it. The upscale prompt notifies from
    /// its own worker thread, so its entry is split off: where it lands among the main thread's
    /// calls is not part of the behaviour.
    fn take_log(&self, notifies: usize) -> (Vec<String>, Vec<String>) {
        let is_notify = |s: &String| s.starts_with("notify ");
        let mut log = self.log.lock().unwrap();
        while log.iter().filter(|s| is_notify(s)).count() < notifies {
            log = self.logged.wait(log).unwrap();
        }
        std::mem::take(&mut *log)
            .into_iter()
            .partition(|s| !is_notify(s))
    }
}

impl Sys for Fake {
    fn hypr_json(&self, _: &str) -> Option<serde_json::Value> {
        serde_json::from_str(r#"[{"name":"DP-1","width":1920,"height":1080,"x":0,"y":0,"scale":1.0,"focused":true}]"#).ok()
    }
    fn gsettings_get(&self, _: &str) -> Option<String> {
        Some("prefer-dark".into())
    }
    fn gsettings_set(&self, key: &str, value: &str) {
        self.log(format!("gsettings set {key} {value}"));
    }
    fn run(&self, argv: &[String], _: Option<&Path>) -> Option<String> {
        self.log(format!("run {}", argv.join(" ")));
        None
    }
    fn spawn(&self, argv: &[String]) {
        self.log(format!("spawn {}", argv.join(" ")));
    }
    fn has_command(&self, _: &str) -> bool {
        true
    }
    fn kill_exact_wait(&self, name: &str, sig: i32) {
        self.log(format!("pkill -{sig} -x {name}; pidwait -x {name}"));
    }
    fn signal_named(&self, name: &str, sig: i32) {
        self.log(format!("signal {name} {sig}"));
    }
    fn ttys(&self) -> Vec<PathBuf> {
        vec![]
    }
    fn write_tty(&self, _: &Path, _: &[u8]) {}
    fn notify(
        &self,
        app: &str,
        _: Option<&str>,
        summary: &str,
        body: &str,
        _: &[(&str, &str)],
        then: colorgen::sys::OnAction,
    ) {
        self.log(format!("notify {app} {summary} {body}"));
        then(None);
    }
}

fn fixture(tag: &str, api: impl Fn(&str, &str) -> Resp + Send + Sync + 'static) -> Option<Fixture> {
    for p in ["bash", "curl", "jq", "awk", "xdg-user-dir"] {
        if !testutil::have(p) {
            return None;
        }
    }
    let dir = testutil::tmpdir(tag);
    let random = dir.join("rusty-impulse/scripts/colors/random");
    std::fs::create_dir_all(&random).unwrap();
    for s in ["random_konachan_wall.sh", "random_osu_wall.sh"] {
        std::fs::copy(
            testutil::unshipped().join("scripts/colors/random").join(s),
            random.join(s),
        )
        .unwrap();
    }
    let sw = dir.join("rusty-impulse/scripts/colors/switchwall.sh");
    crate::testenv::install_script(&sw, SWITCHWALL);
    let srv = testutil::serve(move |r| {
        let base = r
            .header("host")
            .map(|h| format!("http://{h}"))
            .unwrap_or_default();
        api(&base, &r.target)
    });
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let real = std::process::Command::new("bash")
        .args(["-c", "command -v curl"])
        .output()
        .unwrap();
    let real = String::from_utf8_lossy(&real.stdout).trim().to_string();
    let port = srv.port;
    crate::testenv::install_script(
        &bin.join("curl"),
        &format!(
            "#!/bin/bash\nargs=()\nfor a in \"$@\"; do\n  a=\"${{a/#https:\\/\\/konachan.net/http://127.0.0.1:{port}/k}}\"\n  a=\"${{a/#https:\\/\\/osu.ppy.sh/http://127.0.0.1:{port}/o}}\"\n  args+=(\"$a\")\ndone\nexec {real} \"${{args[@]}}\"\n"
        ),
    );
    let home = dir.join("home");
    std::fs::create_dir_all(home.join(".config/rusty-impulse")).unwrap();
    Some(Fixture {
        dir,
        home,
        random,
        bin,
        srv,
        base: None,
    })
}

/// Every file under `d` with its contents, sorted.
fn snapshot(d: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v = vec![];
    fn walk(root: &Path, d: &Path, v: &mut Vec<(String, Vec<u8>)>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                v.push((
                    p.strip_prefix(root).unwrap().display().to_string() + "/",
                    vec![],
                ));
                walk(root, &p, v);
            } else {
                v.push((
                    p.strip_prefix(root).unwrap().display().to_string(),
                    std::fs::read(&p).unwrap_or_default(),
                ));
            }
        }
    }
    walk(d, d, &mut v);
    v.sort();
    v
}

/// Run the original script, then the job, from the same clean HOME; compare.
fn compare(f: &Fixture, site: Site) -> (String, i32) {
    let name = match site {
        Site::Konachan => "random_konachan_wall.sh",
        Site::Osu => "random_osu_wall.sh",
    };
    let keep = snapshot(&f.home);
    let reset = || {
        for (p, b) in snapshot(&f.home) {
            if !keep.iter().any(|(k, _)| *k == p) {
                let p = f.home.join(&p);
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(&p);
                } else {
                    let _ = std::fs::remove_file(&p);
                }
                let _ = b;
            }
        }
    };
    reset();
    let path = format!(
        "{}:{}",
        f.bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let (o1, _, c1) = testutil::run(
        std::process::Command::new("bash")
            .arg("-c")
            .arg(f.random.join(name))
            .env("HOME", &f.home)
            .env("PATH", &path)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_PICTURES_DIR")
            .current_dir(&f.dir),
    );
    let files1 = snapshot(&f.home);
    let reqs1 = f.srv.take();
    reset();
    let base = f.base.clone().unwrap_or_else(|| f.srv.base());
    let ctx = Ctx {
        home: f.home.display().to_string(),
        xdg_config_home: String::new(),
        xdg_pictures_dir: None,
        has_xdg_user_dir: true,
        script_dir: f.random.display().to_string(),
        konachan: format!("{base}/k"),
        osu: format!("{base}/o"),
        ..stub()
    };
    let (o2, _, c2) = testutil::capture(|o, e| run(site, &ctx, o, e));
    let files2 = snapshot(&f.home);
    let reqs2 = f.srv.take();
    assert_eq!(o1, o2, "stdout");
    assert_eq!(c1, c2, "exit code");
    assert_eq!(files1, files2, "files");
    let shape = |rs: &[testutil::Req]| -> Vec<(String, String, Option<String>)> {
        rs.iter()
            .map(|r| {
                let t = match r.target.split_once("&page=") {
                    Some((a, n)) => {
                        let n: u32 = n.parse().unwrap();
                        assert!((1..=1000).contains(&n), "page {n}");
                        format!("{a}&page=N")
                    }
                    None => r.target.clone(),
                };
                let ua = r.header("user-agent").map(String::from);
                (
                    r.method.clone(),
                    t,
                    if site == Site::Osu { None } else { ua },
                )
            })
            .collect()
    };
    assert_eq!(shape(&reqs1), shape(&reqs2), "requests");
    (o1, c1)
}

/// A failed fetch (the scripts would call switchwall.sh with a broken path) leaves the
/// wallpaper alone: no switchwall.sh, no file under Wallpapers, exit code 1.
fn native_fails(f: &Fixture, site: Site) {
    let base = f.base.clone().unwrap_or_else(|| f.srv.base());
    let ctx = Ctx {
        home: f.home.display().to_string(),
        xdg_config_home: String::new(),
        xdg_pictures_dir: None,
        has_xdg_user_dir: true,
        script_dir: f.random.display().to_string(),
        konachan: format!("{base}/k"),
        osu: format!("{base}/o"),
        ..stub()
    };
    let files = || -> Vec<_> {
        snapshot(&f.home)
            .into_iter()
            .filter(|(p, _)| !p.ends_with('/'))
            .collect()
    };
    let before = files();
    let (o, e, c) = testutil::capture(|o, e| run(site, &ctx, o, e));
    assert_eq!(o, "", "switchwall.sh must not run: {e}");
    assert!(e.contains("wallpaper unchanged"), "{e}");
    assert_eq!(c, 1);
    assert_eq!(before, files(), "files");
}

fn konachan(img: &'static str) -> impl Fn(&str, &str) -> Resp + Send + Sync {
    move |base, t| {
        if t.starts_with("/k/post.json?tags=rating%3Asafe&limit=1&page=") {
            Resp::new(
                200,
                format!(r#"[{{"id":1,"file_url":"{base}/img/{img}"}}]"#),
            )
        } else if t == "/img/missing.jpg" {
            Resp::new(404, "not found")
        } else if t == "/img/empty.jpg" {
            Resp::new(200, "")
        } else if t == "/img/challenge.jpg" {
            let mut r = Resp::new(200, "<!DOCTYPE html><title>Just a moment...</title>");
            r.headers
                .push(("Content-Type".into(), "text/html; charset=UTF-8".into()));
            r
        } else if t.starts_with("/img/") {
            Resp::new(200, format!("IMAGE {t}"))
        } else {
            Resp::new(500, "unexpected")
        }
    }
}

fn write_config(f: &Fixture, json: &str) {
    std::fs::write(f.home.join(".config/rusty-impulse/config.json"), json).unwrap();
}

fn user_dirs(f: &Fixture) {
    std::fs::write(
        f.home.join(".config/user-dirs.dirs"),
        "# test\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_PICTURES_DIR=\"$HOME/Изображения\"\n",
    )
    .unwrap();
}

#[test]
fn wall_konachan_equivalent() {
    let Some(f) = fixture("kona", konachan("Konachan.com%20-%201%20sample.jpg")) else {
        return;
    };
    user_dirs(&f);
    write_config(
        &f,
        r#"{"networking":{"userAgent":"UA test/1.0"},"background":{"wallpaperPath":"/x.png"}}"#,
    );
    let (o, _) = compare(&f, Site::Konachan);
    assert!(
        o.contains("Изображения/Wallpapers/random_wallpaper.jpg\n"),
        "{o}"
    );
    assert!(!o.contains("no file"), "{o}");
}

/// curl rejects a link with spaces (exit 3) and the script then sets a missing file.
#[test]
fn wall_konachan_unencoded_link() {
    let Some(f) = fixture("kona-sp", konachan("Konachan.com - 1 sample.jpg")) else {
        return;
    };
    user_dirs(&f);
    native_fails(&f, Site::Konachan);
}

#[test]
fn wall_konachan_current_is_target() {
    let Some(f) = fixture("kona-cur", konachan("a.png")) else {
        return;
    };
    user_dirs(&f);
    let cur = f.home.join("Изображения/Wallpapers/random_wallpaper.png");
    write_config(
        &f,
        &format!(
            r#"{{"background":{{"wallpaperPath":"{}"}}}}"#,
            cur.display()
        ),
    );
    let (o, _) = compare(&f, Site::Konachan);
    assert!(o.contains("random_wallpaper-1.png"), "{o}");
}

#[test]
fn wall_konachan_http_errors_and_empty_body() {
    for img in ["missing.jpg", "empty.jpg", "challenge.jpg"] {
        let Some(f) = fixture("kona-err", konachan(img)) else {
            return;
        };
        user_dirs(&f);
        write_config(&f, r#"{"networking":{"userAgent":""}}"#);
        native_fails(&f, Site::Konachan);
    }
}

#[test]
fn wall_konachan_bad_api_no_config_no_user_dirs() {
    for body in ["garbage", "", "[]", r#"{"a":1}"#, "[{\"file_url\":null}]"] {
        let b = body.to_string();
        let Some(f) = fixture("kona-bad", move |_, _| Resp::new(200, b.clone())) else {
            return;
        };
        native_fails(&f, Site::Konachan);
    }
}

#[test]
fn wall_osu_equivalent() {
    let bodies = [
        r#"{"ends_at":"x","backgrounds":[{"url":"BASE/img/seasonal.jpeg","user":{}}]}"#,
        r#"{"backgrounds":[]}"#,
        r#"{"backgrounds":null}"#,
        "not json",
        "<html><title>Just a moment...</title></html>",
    ];
    for (i, body) in bodies.into_iter().enumerate() {
        let b = body.to_string();
        let Some(f) = fixture("osu", move |base, t| {
            if t == "/o/api/v2/seasonal-backgrounds" {
                Resp::new(200, b.replace("BASE", base))
            } else if t.starts_with("/img/") {
                Resp::new(200, "OSU IMAGE")
            } else {
                Resp::new(500, "unexpected")
            }
        }) else {
            return;
        };
        user_dirs(&f);
        write_config(&f, r#"{"background":{"wallpaperPath":"/elsewhere.jpg"}}"#);
        if i == 0 {
            compare(&f, Site::Osu);
        } else {
            native_fails(&f, Site::Osu);
        }
    }
}

/// konachan unreachable (refused) and a Cloudflare 403 page from either API: the
/// scripts still call switchwall.sh with `random_wallpaper.` (no extension), the job
/// leaves the wallpaper alone.
#[test]
fn wall_network_failure_and_403() {
    for site in [Site::Konachan, Site::Osu] {
        let Some(mut f) = fixture("net-dead", |_, _| Resp::new(500, "unexpected")) else {
            return;
        };
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = dead.local_addr().unwrap().port();
        drop(dead);
        f.base = Some(format!("http://127.0.0.1:{port}"));
        user_dirs(&f);
        native_fails(&f, site);

        let Some(f) = fixture("net-403", |_, _| {
            Resp::new(403, "<!DOCTYPE html><title>Just a moment...</title>")
        }) else {
            return;
        };
        user_dirs(&f);
        write_config(&f, r#"{"networking":{"userAgent":"UA"}}"#);
        native_fails(&f, site);
    }
}

#[test]
fn wall_pictures_dir_without_xdg_user_dir() {
    let d = testutil::tmpdir("pics");
    let mut c = Ctx {
        home: d.display().to_string(),
        xdg_config_home: String::new(),
        xdg_pictures_dir: None,
        has_xdg_user_dir: false,
        script_dir: String::new(),
        konachan: String::new(),
        osu: String::new(),
        ..stub()
    };
    assert_eq!(pictures_dir(&c), format!("{}/Pictures", d.display()));
    std::fs::create_dir_all(d.join(".config")).unwrap();
    std::fs::write(
        d.join(".config/user-dirs.dirs"),
        "XDG_PICTURES_DIR=\"$HOME/P\"\n",
    )
    .unwrap();
    assert_eq!(pictures_dir(&c), format!("{}/P", d.display()));
    c.has_xdg_user_dir = true;
    assert_eq!(pictures_dir(&c), format!("{}/P", d.display()));
    std::fs::write(
        d.join(".config/user-dirs.dirs"),
        "XDG_MUSIC_DIR=\"$HOME/M\"\n",
    )
    .unwrap();
    assert_eq!(pictures_dir(&c), d.display().to_string());
    c.has_xdg_user_dir = false;
    assert_eq!(pictures_dir(&c), "");
}

/// `cargo test --features live -p qml --release wall_bench -- --nocapture --test-threads=1`
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn wall_bench() {
    let Some(f) = fixture("bench", konachan("b.jpg")) else {
        return;
    };
    user_dirs(&f);
    write_config(&f, r#"{"networking":{"userAgent":"UA"}}"#);
    std::fs::write(
        f.dir.join("rusty-impulse/scripts/colors/switchwall.sh"),
        "#!/usr/bin/env bash\necho done\n",
    )
    .unwrap();
    let path = format!(
        "{}:{}",
        f.bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let ctx = Ctx {
        home: f.home.display().to_string(),
        xdg_config_home: String::new(),
        xdg_pictures_dir: None,
        has_xdg_user_dir: true,
        script_dir: f.random.display().to_string(),
        konachan: format!("{}/k", f.srv.base()),
        osu: String::new(),
        ..stub()
    };
    let n = 20;
    let (mut ow, mut oc, mut rw, mut rc) = (0.0, 0.0, 0.0, 0.0);
    for _ in 0..n {
        let (w, c) = testutil::measure_cmd(
            std::process::Command::new("bash")
                .arg("-c")
                .arg(f.random.join("random_konachan_wall.sh"))
                .env("HOME", &f.home)
                .env("PATH", &path)
                .env_remove("XDG_CONFIG_HOME"),
        );
        ow += w;
        oc += c;
        let (w, c) = testutil::measure_thread(|| {
            testutil::capture(|o, e| run(Site::Konachan, &ctx, o, e));
        });
        rw += w;
        rc += c;
    }
    let n = n as f64;
    println!(
        "konachan (incl. stub switchwall.sh): original wall {:.2} ms cpu {:.2} ms | rust wall {:.2} ms cpu(thread+children) {:.2} ms",
        ow / n * 1e3,
        oc / n * 1e3,
        rw / n * 1e3,
        rc / n * 1e3
    );
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_tree(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

/// Put `home` back as `before`, keeping `keep`.
fn restore(home: &Path, before: &[(String, Vec<u8>)], keep: &Path) {
    let mut now = snapshot(home);
    now.reverse();
    for (p, _) in now {
        let full = home.join(p.trim_end_matches('/'));
        if full == keep || keep.starts_with(&full) || before.iter().any(|(b, _)| *b == p) {
            continue;
        }
        if p.ends_with('/') {
            std::fs::remove_dir_all(&full).ok();
        } else {
            std::fs::remove_file(&full).unwrap();
        }
    }
    for (p, data) in before {
        if !p.ends_with('/') {
            std::fs::write(home.join(p), data).unwrap();
        }
    }
}

/// The job's last step is `colorgen::switchwall` in-process: `switchwall.sh` never runs, and
/// the job prints, returns, asks of the session and writes exactly what the port does when run
/// as `switchwall.sh --image <file>` (whose equivalence with the script is colorgen's tests).
#[test]
fn wall_runs_switchwall_in_process() {
    let img: &'static [u8] =
        include_bytes!("../../../../colorgen/tests/switchwall/fixture/thumb.jpg");
    let dir = testutil::tmpdir("sw-native");
    let colors = dir.join("rusty-impulse/scripts/colors");
    let random = colors.join("random");
    std::fs::create_dir_all(&random).unwrap();
    copy_tree(
        &testutil::tree().join("scripts/colors/terminal"),
        &colors.join("terminal"),
    );
    let mark = dir.join("switchwall.sh-ran");
    let sw = colors.join("switchwall.sh");
    crate::testenv::install_script(&sw, &format!("#!/bin/sh\ntouch '{}'\n", mark.display()));
    let srv = testutil::serve(move |r| {
        let base = r
            .header("host")
            .map(|h| format!("http://{h}"))
            .unwrap_or_default();
        if r.target.starts_with("/k/post.json") {
            Resp::new(200, format!(r#"[{{"file_url":"{base}/img/w.jpg"}}]"#))
        } else if r.target == "/img/w.jpg" {
            Resp::new(200, img)
        } else {
            Resp::new(500, "unexpected")
        }
    });
    let home = dir.join("home");
    for d in [".config/rusty-impulse", ".config/rustshell/rusty-impulse"] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    std::fs::write(
        home.join(".config/rusty-impulse/config.json"),
        r#"{"background":{"wallpaperPath":"/x.png"},"appearance":{"wallpaperTheming":{"enableTerminal":true}}}"#,
    )
    .unwrap();
    let fake = Arc::new(Fake::default());
    let ctx = Ctx {
        home: home.display().to_string(),
        has_xdg_user_dir: false,
        script_dir: random.display().to_string(),
        konachan: format!("{}/k", srv.base()),
        switchwall: native_switchwall,
        sys: fake.clone(),
        ..stub()
    };
    let before = snapshot(&home);
    let job = testutil::capture(|o, e| run(Site::Konachan, &ctx, o, e));
    let job_files = snapshot(&home);
    let job_log = fake.take_log(1);
    assert!(!mark.exists(), "switchwall.sh ran");

    let wall = home.join("Pictures/Wallpapers/random_wallpaper.jpg");
    assert_eq!(std::fs::read(&wall).unwrap(), img);
    restore(&home, &before, &wall);
    let p = colorgen::switchwall::Paths {
        config_home: home.join(".config"),
        cache_home: home.join(".cache"),
        state_home: home.join(".local/state"),
        script_dir: colors.clone(),
        home: home.clone(),
    };
    let sys: Arc<dyn Sys> = fake.clone();
    let args = ["--image".to_string(), wall.display().to_string()];
    let direct = testutil::capture(|out, err| {
        colorgen::switchwall::main_io(&args, &p, &sys, Io { out, err })
    });
    let direct_log = fake.take_log(1);

    assert_eq!(job, direct);
    assert_eq!(job_log, direct_log);
    assert_eq!(job_files, snapshot(&home));
    assert!(
        job_files
            .iter()
            .any(|(p, _)| p.ends_with("user/generated/material_colors.scss")),
        "colorgen did not run: {job:?}"
    );
}

/// The extension comes from the link's file name and is a short alphanumeric word, else `jpg`:
/// the server cannot pick a path (`random_wallpaper.com/a`) or put a query in the name.
#[test]
fn wallpaper_extension_is_sanitized() {
    for (link, ext) in [
        (
            "https://konachan.net/img/Konachan.com%20-%201%20sample.jpg",
            "jpg",
        ),
        ("https://osu.ppy.sh/x/seasonal.jpeg", "jpeg"),
        ("https://x.com/a", "jpg"),
        ("https://x.com/a.png?v=1.2/x#y.z", "png"),
        ("https://x.com/a.toolongext", "jpg"),
        ("https://x.com/a.p%2Fg", "jpg"),
        ("", "jpg"),
    ] {
        assert_eq!(ext_of(link), ext, "{link}");
    }
}

/// A wallpaper API answering with an endless body (or a `Content-Length` over the limit) gets
/// an error and leaves no file, not a disk filled through `random_wallpaper.jpg.part`.
#[test]
fn wallpaper_download_is_bounded() {
    use std::io::{BufRead, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut r = std::io::BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let big = line.contains("/big");
                while r.read_line(&mut line).is_ok_and(|n| n > 2) {
                    line.clear();
                }
                let mut s = s;
                let head = if big {
                    "HTTP/1.1 200 OK\r\nContent-Length: 1099511627776\r\n\r\n"
                } else {
                    "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n"
                };
                let _ = s.write_all(head.as_bytes());
                let chunk = vec![1u8; 1 << 20];
                while s.write_all(&chunk).is_ok() {}
            });
        }
    });
    let d = testutil::tmpdir("wall-max");
    for route in ["big", "endless"] {
        let path = d.join("random_wallpaper.jpg");
        let e = download(
            &format!("http://127.0.0.1:{port}/{route}"),
            None,
            path.to_str().unwrap(),
        )
        .unwrap_err();
        assert!(e.contains("over"), "{route}: {e}");
        let left: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert!(left.is_empty(), "{route}: {left:?}");
    }
}

/// The original's `services/Switchwall.qml` `picker` command (switchwall.sh's own prompt lines).
const PICKER: &str = "cd \"$(xdg-user-dir PICTURES)/Wallpapers/showcase\" 2>/dev/null || cd \"$(xdg-user-dir PICTURES)/Wallpapers\" 2>/dev/null || cd \"$(xdg-user-dir PICTURES)\" || exit 1; kdialog --getopenfilename . --title 'Choose wallpaper'";

/// A `kdialog` that prints where it was opened and with what, then exits with `bin/code`
/// (a cancelled dialog prints nothing and exits 1), and an `xdg-user-dir` naming `bin/pics`.
fn picker_bin(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    crate::testenv::install_script(
        &bin.join("kdialog"),
        "#!/usr/bin/env bash\nc=$(cat \"$(dirname \"$0\")/code\")\n[ \"$c\" = 0 ] && echo \"$PWD/picked one.png $*\"\nexit \"$c\"\n",
    );
    crate::testenv::install_script(
        &bin.join("xdg-user-dir"),
        "#!/usr/bin/env bash\ncat \"$(dirname \"$0\")/pics\"\n",
    );
    bin
}

/// The picker job's stdout and exit code.
fn run_picker(job: Job, stop: Arc<crate::svc::Stop>) -> (String, i32) {
    let (tx, rx) = crate::svc::Waker::default()
        .handle(crate::svc::Svc::Process)
        .channel();
    crate::jobs::start(job, tx, stop);
    let mut out = String::new();
    for ev in rx {
        match ev {
            crate::native::ProcEv::Out(s) => out.push_str(&s),
            crate::native::ProcEv::Err(_) | crate::native::ProcEv::File { .. } => {}
            crate::native::ProcEv::Exit(c) => return (out, c),
            crate::native::ProcEv::Signaled(_) => unreachable!("a job exits"),
        }
    }
    unreachable!("a job always reports its exit")
}

/// `services/Switchwall.qml`'s `picker` against its original: the original's bash command line with
/// the same `kdialog` and `xdg-user-dir` stubs. The dialog opens in the same folder (showcase,
/// Wallpapers, Pictures, in that order), with the same arguments, the same stdout and exit
/// code; no Pictures folder at all exits 1 without a dialog, as the failed `cd` does.
#[test]
fn switchwall_picker_matches_bash() {
    let dir = testutil::tmpdir("switchwall-picker");
    let bin = picker_bin(&dir);
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let cases: &[(&str, &[&str], &str)] = &[
        ("showcase", &["Wallpapers/showcase"], "0"),
        ("wallpapers", &["Wallpapers"], "0"),
        ("pictures", &[""], "0"),
        ("cancelled", &["Wallpapers"], "1"),
        ("no pictures", &[], "0"),
    ];
    for (name, layout, code) in cases {
        let pics = dir.join(name.replace(' ', "-")).join("Pictures");
        for d in *layout {
            std::fs::create_dir_all(pics.join(d)).unwrap();
        }
        std::fs::write(bin.join("code"), code).unwrap();
        std::fs::write(bin.join("pics"), pics.display().to_string()).unwrap();
        let (bash_out, _, bash_code) = testutil::run(
            std::process::Command::new("bash")
                .args(["-c", PICKER])
                .env("PATH", &path),
        );
        let port = run_picker(
            picker(pics.clone(), bin.join("kdialog")),
            Default::default(),
        );
        assert_eq!(port, (bash_out, bash_code), "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Stopping the `services/Switchwall.qml` `picker` (the shell going away) closes the dialog it
/// opened instead of leaving it behind: the job ends as the dialog dies of SIGTERM.
#[test]
fn switchwall_picker_stop_closes_dialog() {
    let dir = testutil::tmpdir("switchwall-picker-stop");
    let pics = dir.join("Pictures");
    std::fs::create_dir_all(&pics).unwrap();
    let kdialog = dir.join("kdialog");
    let fifo = dir.join("opened");
    crate::testenv::install_script(
        &kdialog,
        &format!(
            "#!/usr/bin/env bash\necho open > '{}'\nexec tail -f /dev/null\n",
            fifo.display()
        ),
    );
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let stop: Arc<crate::svc::Stop> = Default::default();
    let (tx, rx) = std::sync::mpsc::channel();
    let s = stop.clone();
    let job = picker(pics, kdialog);
    std::thread::spawn(move || tx.send(run_picker(job, s)).unwrap());
    // Blocks until the dialog is up.
    assert_eq!(std::fs::read_to_string(&fifo).unwrap(), "open\n");
    stop.store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(rx.recv().unwrap(), (String::new(), 128 + libc::SIGTERM));
    let _ = std::fs::remove_dir_all(&dir);
}
