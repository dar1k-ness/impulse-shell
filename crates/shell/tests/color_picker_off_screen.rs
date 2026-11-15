//! The waffle region selector's colour picker (WRegionSelectionPanel.qml) starts hyprpicker,
//! which freezes the screen as it starts, only once the closed panel is off screen: after the
//! panel's surface was destroyed and the compositor presented a frame without it, not after the
//! original's `sleep 0.2` guess. The panel here fades out over 2 s after `closed()`, as a panel's
//! exit animation keeps it on screen; a fake `hyprpicker` on PATH screenshots the output (what
//! the real one freezes) as it starts and reports its arguments through a FIFO.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;

/// WRegionSelectionPanel.qml's color picker `onClicked` body, as the tree has it.
fn picker() -> String {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../qml/modules/waffle/screenSnip/WRegionSelectionPanel.qml"
    );
    let doc = qml::parse(&std::fs::read_to_string(file).unwrap()).unwrap();
    let mut v = vec![];
    doc.root.walk(&mut |o| o.scripts(&mut v));
    let s = v
        .iter()
        .find(|s| s.src.contains("ColorPicker.pickOnceHidden("));
    s.expect("no colour picker body").src.to_string()
}

/// The tree: a bar on the output `bar` and the panel that closes on the output `screen`.
fn shell_qml(bar: &str, screen: &str) -> String {
    let picker = picker();
    format!(
        r##"import QtQuick
import Rustshell
import Rustshell.Wayland
import Impulse
ShellRoot {{
    id: shell
    property bool snip: true
    property bool bar: true
    property bool locked: false
    WlSessionLock {{
        locked: shell.locked
        surface: Component {{
            WlSessionLockSurface {{ color: "black" }}
        }}
    }}
    PanelWindow {{
        visible: shell.bar
        screen: Rustshell.screens.find(s => s.name === "{bar}") ?? null
        anchors {{ top: true; left: true; right: true }}
        implicitHeight: 24
        exclusionMode: ExclusionMode.Ignore
        color: "#202020"
    }}
    PanelWindow {{
        id: root
        signal closed()
        visible: shell.snip
        screen: Rustshell.screens.find(s => s.name === "{screen}") ?? null
        onClosed: fade.start()
        anchors {{ top: true; bottom: true; left: true; right: true }}
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        Rectangle {{ id: fill; anchors.fill: parent; color: "#ff0000" }}
        NumberAnimation {{
            id: fade; target: fill; property: "opacity"; to: 0; duration: 2000
            onFinished: shell.snip = false
        }}
        Item {{
            id: picker
            signal clicked()
            onClicked: {picker}
        }}
    }}
    IpcHandler {{
        target: "fx"
        function pick(): void {{ picker.clicked() }}
        function hide(): void {{ shell.snip = false }}
        function show(): void {{ shell.snip = true }}
        function lock(): void {{ shell.locked = true }}
        function unlock(): void {{ shell.locked = false }}
        function now(): void {{ ColorPicker.pickOnceHidden(root); shell.snip = false }}
    }}
}}
"##
    )
}

fn sway() -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("a headless sway (WAYLAND_DISPLAY)");
        return false;
    }
    testenv::require_all(&["mkfifo", "grim"])
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "hyprpicker_starts_once_the_closed_panel_is_off_screen",
        &[("WAYLAND_DISPLAY", None)],
    );
}

struct Shell {
    child: Child,
    dir: tempfile::TempDir,
    log: Arc<Log>,
    /// The Wayland socket it is on, when not the environment's.
    display: Option<PathBuf>,
}

impl Shell {
    /// impulse-shell on `shell_qml(SCREEN)`, with `bin` first on PATH, drawn and idle; on the Wayland
    /// socket `display` instead of the environment's when given.
    fn start(bin: &Path, display: Option<&Path>) -> Self {
        Self::start_with(bin, display, SCREEN)
    }

    /// Like `start`, with the panel on the output `screen`.
    fn start_with(bin: &Path, display: Option<&Path>, screen: &str) -> Self {
        Self::start_on(bin, display, SCREEN, screen)
    }

    /// Like `start`, with the bar on the output `bar` and the panel on the output `screen`.
    fn start_on(bin: &Path, display: Option<&Path>, bar: &str, screen: &str) -> Self {
        Self::start_in(bin, display, bar, screen, None)
    }

    /// Like `start`, in the session of the Hyprland `hypr`.
    fn start_under(bin: &Path, display: &Path, hypr: &FakeHypr) -> Self {
        Self::start_in(bin, Some(display), SCREEN, SCREEN, Some(hypr))
    }

    fn start_in(
        bin: &Path,
        display: Option<&Path>,
        bar: &str,
        screen: &str,
        hypr: Option<&FakeHypr>,
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("shell.qml"), shell_qml(bar, screen)).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        let xdg = dir.path().join("xdg");
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        let child = Command::new(env!("CARGO_BIN_EXE_impulse-shell"))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env("RI_LOG", "ri::frames=debug,ri::sched=debug")
            .env("PATH", path)
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
            .envs(hypr.map(|h| ("HYPRLAND_INSTANCE_SIGNATURE", &h.signature)))
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .stdout(Stdio::null())
            .stderr(w)
            .envs(display.map(|d| ("WAYLAND_DISPLAY", d)))
            .spawn()
            .unwrap();
        let sh = Shell {
            child,
            dir,
            log: Log::capture(r),
            display: display.map(Path::to_path_buf),
        };
        sh.wait("both panels drawn", |s| {
            s.contains("draw win0") && s.contains("draw win1")
        });
        // Mapped (their first commits' wl_display.sync answered): the loop has nothing left to do.
        sh.wait_blocked_after("the loop idle after the first frames", |l| {
            l.contains("draw win")
        });
        sh
    }

    /// Waits for `f(log)` ([`Log::wait`] ends a wait the shell can no longer satisfy).
    fn wait(&self, what: &str, f: impl Fn(&str) -> bool) {
        if let Err(s) = self.log.wait(f) {
            let keep: Vec<&str> = s
                .lines()
                .filter(|l| {
                    l.contains("ri::frames")
                        || l.contains("qml:")
                        || l.contains("layer_surface")
                        || l.contains("presentation")
                })
                .collect();
            panic!(
                "timed out waiting for {what}; impulse-shell log tail:\n{}",
                keep[keep.len().saturating_sub(40)..].join("\n")
            );
        }
    }

    /// Waits until the loop blocks with nothing due (ri::sched logs each turn's next deadline as
    /// `t=… next …`) after the last line `mark` matches.
    fn wait_blocked_after(&self, what: &str, mark: impl Fn(&str) -> bool) {
        self.wait(what, |s| {
            let lines: Vec<&str> = s.lines().collect();
            let turn = lines
                .iter()
                .rposition(|l| l.trim_start().starts_with("t=") && l.contains(" next "));
            let m = lines.iter().rposition(|l| mark(l));
            turn.is_some_and(|t| lines[t].contains(" next None") && m.is_none_or(|m| m < t))
        });
    }

    fn ipc(&self, func: &str) {
        let st = Command::new(env!("CARGO_BIN_EXE_impulse-shell"))
            .arg("-p")
            .arg(self.dir.path())
            .args(["ipc", "call", "fx", func])
            .envs(self.display.as_deref().map(|d| ("WAYLAND_DISPLAY", d)))
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "ipc call fx {func}");
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The RGB pixel at `x,y` of the binary PPM at `f`.
fn pixel(f: &Path, x: usize, y: usize) -> [u8; 3] {
    let b = std::fs::read(f).unwrap();
    // P6\n<w> <h>\n255\n<rgb>
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while b[i].is_ascii_whitespace() {
            i += 1;
        }
        let s = i;
        while !b[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(String::from_utf8_lossy(&b[s..i]).into_owned());
    }
    let w: usize = fields[1].parse().unwrap();
    let o = i + 1 + (y * w + x) * 3;
    [b[o], b[o + 1], b[o + 2]]
}

/// The one output of the shared sway.
const SCREEN: &str = "HEADLESS-1";

/// The shell command that screenshots `output` to a binary PPM (the file follows).
fn grim(output: &str) -> String {
    format!("grim -t ppm -o {output}")
}

/// Whether the centre of the screenshot `shot` shows the closed panel's red fill.
fn panel_on(shot: &Path) -> Option<[u8; 3]> {
    let [r, g, b] = pixel(shot, 960, 540);
    (r > g.max(b).saturating_add(16)).then_some([r, g, b])
}

/// A directory with a fake `hyprpicker` that screenshots `output` to `shot.ppm` in it as it
/// starts, and its arguments once it has.
fn fake_hyprpicker(output: &str) -> (tempfile::TempDir, std::sync::mpsc::Receiver<String>) {
    let bin = tempfile::tempdir().unwrap();
    let shot = format!(
        "{} '{}'",
        grim(output),
        bin.path().join("shot.ppm").display()
    );
    let started = fake_picker(bin.path(), &shot);
    (bin, started)
}

/// A fake `hyprpicker` in `bin` that notes its start in `starts` there, runs `first`, and then
/// hands over its arguments, each time it is started.
fn fake_picker(bin: &Path, first: &str) -> std::sync::mpsc::Receiver<String> {
    let fifo = bin.join("started");
    let st = Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(st.success(), "mkfifo");
    testenv::install_script(
        &bin.join("hyprpicker"),
        &format!(
            "#!/bin/sh\ndate +%s%N >> '{}'\n{first}\necho \"$*\" > '{}'\n",
            bin.join("starts").display(),
            fifo.display()
        ),
    );
    let (tx, started) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(args) = std::fs::read_to_string(&fifo) {
            if tx.send(args).is_err() {
                break;
            }
        }
    });
    started
}

/// How many times the fake hyprpicker in `bin` was started.
fn starts(bin: &Path) -> usize {
    let s = std::fs::read_to_string(bin.join("starts")).unwrap_or_default();
    s.lines().count()
}

/// The shell's probes in its Wayland log (the layer surfaces it maps to hear of a frame):
/// how many it made, and how many of their layer surfaces and wl_surfaces the compositor
/// still has (its wl_display.delete_id for one comes once it has destroyed it).
fn probes(log: &str) -> (usize, usize) {
    let (mut made, mut live) = (0, std::collections::HashSet::new());
    for l in log.lines() {
        if let Some((_, r)) = l.split_once("get_layer_surface(new id ")
            && r.ends_with("\"impulse-shell-shown\")")
        {
            made += 1;
            for id in r.split(", ").take(2) {
                live.insert(id.split_once('#').unwrap().1.to_string());
            }
        } else if let Some((_, r)) = l.split_once("wl_display#1.delete_id(") {
            live.remove(r.trim_end_matches(')'));
        }
    }
    (made, live.len())
}

/// A probe's surface entered an output: the compositor has mapped it.
fn probe_shown(log: &str) -> bool {
    let mut probe = std::collections::HashSet::new();
    log.lines().any(|l| {
        if let Some((_, r)) = l.split_once("get_layer_surface(new id ")
            && r.ends_with("\"impulse-shell-shown\")")
        {
            probe.insert(r.split(", ").nth(1).unwrap().to_string());
        }
        l.split_once(".enter(wl_output#")
            .and_then(|(l, _)| l.split_whitespace().last())
            .is_some_and(|surface| probe.contains(surface))
    })
}

/// How many times a window of the shell's (not a probe) entered an output.
fn entered(log: &str) -> usize {
    let mut probe = std::collections::HashSet::new();
    let mut n = 0;
    for l in log.lines() {
        if let Some((_, r)) = l.split_once("get_layer_surface(new id ") {
            let surface = r.split(", ").nth(1).unwrap().to_string();
            if r.ends_with("\"impulse-shell-shown\")") {
                probe.insert(surface);
            } else {
                probe.remove(&surface);
            }
        } else if let Some((l, _)) = l.split_once(".enter(wl_output#")
            && let Some(surface) = l.split_whitespace().last()
            && surface.starts_with("wl_surface#")
            && !probe.contains(surface)
        {
            n += 1;
        }
    }
    n
}

/// Waits until the bar and the panel are on their outputs: a panel closed before the
/// compositor said which output it is on is waited for on none.
fn wait_on_outputs(sh: &Shell) {
    sh.wait("the bar and the panel on the output", |s| entered(s) >= 2);
}

/// The log line of work dropped because of `why`.
fn dropped(why: &str) -> String {
    format!("off screen: dropped the work of a closed window: {why}")
}

/// hyprpicker starts, as the tree starts it, with the closed panel off the screen it freezes.
fn assert_starts_off_screen(bin: &Path, started: &std::sync::mpsc::Receiver<String>) {
    let args = started.recv().expect("hyprpicker never started");
    assert_eq!(args, "-a\n");
    let on = panel_on(&bin.join("shot.ppm"));
    assert_eq!(
        on, None,
        "hyprpicker started with the closed panel still on screen"
    );
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_once_the_closed_panel_is_off_screen() {
    if !sway() {
        return;
    }
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), None);
    sh.ipc("pick");
    assert_starts_off_screen(bin.path(), &started);
}

/// The compositor has destroyed the first surface the shell destroyed (the closed panel's):
/// its wl_display.delete_id came.
fn panel_surface_gone(log: &str) -> bool {
    let mut lines = log.lines();
    let id = lines.find_map(|l| {
        let (_, s) = l.split_once("-> wl_surface#")?;
        s.strip_suffix(".destroy()")
    });
    id.is_some_and(|id| {
        let gone = format!("wl_display#1.delete_id({id})");
        lines.any(|l| l.ends_with(&gone))
    })
}

/// A shell kept off the CPU by other work hears what became of its commits only after the
/// compositor repainted without the panel. What it waits for is the answer about a commit
/// that repaint showed, which comes however late the shell reads it: a commit made on hearing
/// of the repaint has nothing new for the compositor to repaint for, and is never answered.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_when_the_shell_hears_of_its_commits_after_the_repaint() {
    if !sway() {
        return;
    }
    let px = testenv::HeldFeedback::start();
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), Some(px.path()));
    px.hold();
    sh.ipc("pick");
    sh.wait(
        "the compositor destroying the panel's surface",
        panel_surface_gone,
    );
    sh.wait("the probe on the output", probe_shown);
    // grim gets the output's next frame: the compositor has repainted without the panel.
    let shot = bin.path().join("repainted.ppm");
    let st = Command::new("sh")
        .arg("-c")
        .arg(format!("{} '{}'", grim(SCREEN), shot.display()))
        .status()
        .unwrap();
    assert!(st.success(), "grim");
    assert_eq!(panel_on(&shot), None, "the panel is still on screen");
    px.release();
    assert_starts_off_screen(bin.path(), &started);
    // The probe was on that frame and is gone from the one hyprpicker got: it shows nothing.
    assert!(
        std::fs::read(&shot).unwrap() == std::fs::read(bin.path().join("shot.ppm")).unwrap(),
        "the probe changed the screen"
    );
}

/// The compositor answers nothing about the commits of the windows the shell has left: it
/// does not draw them, as Hyprland does not draw a bar under a fullscreen window. The picker
/// does not wait for a frame of theirs.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_when_the_compositor_does_not_draw_the_windows_left() {
    if !sway() {
        return;
    }
    let px = testenv::HeldFeedback::start();
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), Some(px.path()));
    px.silence_surfaces();
    sh.ipc("pick");
    assert_starts_off_screen(bin.path(), &started);
}

/// The panel is hidden already when the picker is asked for: it is on no output, so there is
/// no frame to wait for, only the compositor's word that it has read the shell's requests
/// (wl_display.sync).
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_for_a_panel_that_is_not_shown() {
    if !sway() {
        return;
    }
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), None);
    sh.ipc("hide");
    sh.wait(
        "the compositor destroying the panel's surface",
        panel_surface_gone,
    );
    sh.ipc("pick");
    assert_starts_off_screen(bin.path(), &started);
    let log = sh.log.text();
    assert!(
        !log.lines()
            .any(|l| l.contains("-> wp_presentation#") && l.contains(".feedback(")),
        "the shell asked when a frame is shown, with no output to show one on:\n{log}"
    );
}

fn swaymsg(sock: &Path, args: &[&str]) -> String {
    let out = Command::new("swaymsg")
        .arg("-s")
        .arg(sock)
        .args(args)
        .output()
        .expect("swaymsg");
    assert!(
        out.status.success(),
        "swaymsg {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn output_names(sock: &Path) -> Vec<String> {
    let v: serde_json::Value =
        serde_json::from_str(&swaymsg(sock, &["-r", "-t", "get_outputs"])).unwrap();
    let names = v.as_array().unwrap().iter();
    names
        .filter_map(|o| o["name"].as_str().map(str::to_string))
        .collect()
}

/// A second headless output of the shared sway; unplugged on drop.
struct Output {
    sock: PathBuf,
    name: String,
    plugged: bool,
}

impl Output {
    fn create() -> Self {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR");
        let sock = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("sway-ipc.") && n.ends_with(".sock"))
            })
            .expect("no sway IPC socket (this test needs sway)");
        let before = output_names(&sock);
        swaymsg(&sock, &["create_output"]);
        let name = output_names(&sock)
            .into_iter()
            .find(|n| !before.contains(n))
            .expect("create_output added no output");
        Output {
            sock,
            name,
            plugged: true,
        }
    }

    fn unplug(&mut self) {
        if std::mem::take(&mut self.plugged) {
            swaymsg(&self.sock, &["output", &self.name, "unplug"]);
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.unplug();
    }
}

/// The closed panel was the shell's only window on its output. The bar, on the other one, is
/// not asked when its commit is shown: nothing changed on its output, so the compositor does
/// not repaint it and never answers.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_when_the_panel_was_alone_on_its_output() {
    if !sway() || !testenv::require_all(&["swaymsg"]) {
        return;
    }
    let second = Output::create();
    let (bin, started) = fake_hyprpicker(&second.name);
    let sh = Shell::start_with(bin.path(), None, &second.name);
    sh.ipc("pick");
    assert_starts_off_screen(bin.path(), &started);
}

/// The output the closed panel was on is unplugged before the compositor answers: nothing is
/// shown on that output any more, so the wait for a frame there ends.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_when_the_output_it_waited_on_is_unplugged() {
    if !sway() || !testenv::require_all(&["swaymsg"]) {
        return;
    }
    let px = testenv::HeldFeedback::start();
    let mut second = Output::create();
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start_on(bin.path(), Some(px.path()), &second.name, &second.name);
    px.hold();
    sh.ipc("pick");
    sh.wait("the shell asking when the output shows a frame", |s| {
        s.lines()
            .any(|l| l.contains("-> wp_presentation#") && l.contains(".feedback("))
    });
    second.unplug();
    let args = started.recv().expect("hyprpicker never started");
    assert_eq!(args, "-a\n");
}

/// The panel closes in the turn that asks for the picker, so the pass that destroys its
/// surface is the first to see the work: the shell still knows which output the panel was on
/// and waits for a frame shown there, not only for the compositor to hear of the destruction.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn panel_closed_in_the_turn_that_asks_is_waited_for_on_its_output() {
    if !sway() {
        return;
    }
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), None);
    sh.wait("the bar and the panel on the output", |s| {
        s.lines()
            .filter(|l| l.contains(".enter(wl_output#"))
            .count()
            == 2
    });
    sh.ipc("now");
    assert_starts_off_screen(bin.path(), &started);
    let log = sh.log.text();
    assert!(
        log.lines()
            .any(|l| l.contains("-> wp_presentation#") && l.contains(".feedback(")),
        "the shell did not ask when a frame is shown:\n{log}"
    );
}

/// A compositor that animates a closed window away keeps showing the window's last frame
/// meanwhile, and tells no client when it stops (Hyprland's `layersOut` and `fadeLayersOut`;
/// here the compositor never hears that the panel's surface is destroyed, so it shows it for
/// good). The panel's last frame is empty: nothing of it is on the screen hyprpicker freezes.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn nothing_is_left_of_a_closed_panel_the_compositor_still_shows() {
    if !sway() {
        return;
    }
    let px = testenv::HeldFeedback::start();
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), Some(px.path()));
    wait_on_outputs(&sh);
    px.keep_surfaces_shown();
    sh.ipc("now");
    assert_starts_off_screen(bin.path(), &started);
}

/// The compositor composes its frame without the shell's probe and says so (`discarded`), as
/// Hyprland does for a surface it rounds to one device pixel. No other commit of the probe
/// asks, so nothing more is coming: the wait ends there, and the probe is destroyed.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_when_the_frame_of_the_probe_is_discarded() {
    if !sway() {
        return;
    }
    let px = testenv::HeldFeedback::start();
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), Some(px.path()));
    wait_on_outputs(&sh);
    px.discard_for_new_surfaces();
    sh.ipc("now");
    assert_starts_off_screen(bin.path(), &started);
    sh.wait("the probe destroyed", |s| probes(s) == (1, 0));
    let log = sh.log.text();
    assert!(
        log.lines()
            .any(|l| l.contains("wp_presentation_feedback#") && l.ends_with(".discarded()")),
        "the probe's frame was not discarded:\n{log}"
    );
}

/// The compositor that composed its frame without the probe composes no other one for it,
/// and sway configures the probe a second time once it is mapped. A commit that only
/// answers that configure changes nothing, so feedback asked for on it would never come:
/// the probe asks once, on the commit that carries its buffer, and that `discarded` ends
/// the wait.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_when_no_frame_follows_the_discarded_one_of_the_probe() {
    if !sway() {
        return;
    }
    let px = testenv::HeldFeedback::start();
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), Some(px.path()));
    wait_on_outputs(&sh);
    px.discard_once_for_new_surfaces();
    sh.ipc("now");
    assert_starts_off_screen(bin.path(), &started);
    sh.wait("the probe destroyed", |s| probes(s) == (1, 0));
    let log = sh.log.text();
    let probe = log
        .lines()
        .find_map(|l| {
            let (_, r) = l.split_once("get_layer_surface(new id ")?;
            r.ends_with("\"impulse-shell-shown\")")
                .then(|| r.split_once(',').unwrap().0.to_string())
        })
        .unwrap();
    let configures = log
        .lines()
        .filter(|l| l.contains(&format!(" {probe}.configure(")))
        .count();
    assert!(
        configures >= 2,
        "sway did not configure the probe a second time:\n{log}"
    );
}

/// A hundred pickers one after another leave no probe behind: the compositor has destroyed
/// each one's layer surface and wl_surface by the time the next panel is shown.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_hundred_pickers_leave_no_probe() {
    if !sway() {
        return;
    }
    let bin = tempfile::tempdir().unwrap();
    let started = fake_picker(bin.path(), "");
    let sh = Shell::start(bin.path(), None);
    for n in 0..100 {
        // The bar, and the panel each time it is shown, are on the output.
        sh.wait("the panel on the output", |s| entered(s) == 2 + n);
        sh.ipc("now");
        assert_eq!(started.recv().expect("hyprpicker never started"), "-a\n");
        sh.wait("the probe destroyed", |s| probes(s) == (n + 1, 0));
        sh.ipc("show");
    }
    assert_eq!(starts(bin.path()), 100);
}

/// The picker works at the output scales and rotation a probe must be drawn at.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_on_a_scaled_or_rotated_output() {
    if !testenv::require_all(&["sway", "swaymsg", "mkfifo", "grim"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let sock = sway.ipc_socket();
    for (scale, transform) in [
        ("1", "normal"),
        ("1.25", "normal"),
        ("1.5", "normal"),
        ("2", "normal"),
        ("1", "90"),
        ("1.5", "90"),
    ] {
        swaymsg(&sock, &["output", SCREEN, "scale", scale]);
        swaymsg(&sock, &["output", SCREEN, "transform", transform]);
        let (bin, started) = fake_hyprpicker(SCREEN);
        let sh = Shell::start(bin.path(), Some(sway.display()));
        wait_on_outputs(&sh);
        sh.ipc("now");
        let args = started.recv();
        assert_eq!(
            args.as_deref(),
            Ok("-a\n"),
            "scale {scale} transform {transform}"
        );
        let shot = bin.path().join("shot.ppm");
        // A pixel inside the screenshot at every scale and rotation here.
        let [r, g, b] = pixel(&shot, 500, 500);
        assert!(
            r <= g.max(b).saturating_add(16),
            "the panel is on screen at scale {scale} transform {transform}"
        );
        sh.wait("the probe destroyed", |s| probes(s) == (1, 0));
    }
}

/// The session is locked (by this shell's own lock) when the picker is asked for: only the
/// lock surfaces are shown, a probe would be drawn after the unlock, and hyprpicker would
/// start then, long after it was asked for. It is not started at all.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_does_not_start_in_a_locked_session() {
    if !testenv::require_all(&["sway", "mkfifo", "grim"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), Some(sway.display()));
    wait_on_outputs(&sh);
    sh.ipc("lock");
    sh.wait("the session locked", |s| {
        s.lines().any(|l| l.ends_with(".locked()"))
    });
    sh.ipc("now");
    let why = dropped("the session is locked");
    sh.wait("the picker's work dropped", |s| s.contains(&why));
    assert_eq!(probes(&sh.log.text()).0, 0, "a probe in a locked session");
    assert_picker_works_once_more(&sh, bin.path(), &started);
}

/// The session is locked while the picker waits for its frame: the wait ends there, with the
/// probe destroyed, and hyprpicker does not start after the unlock.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn locking_the_session_ends_the_wait_for_a_frame() {
    if !testenv::require_all(&["sway", "mkfifo", "grim"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let px = testenv::HeldFeedback::start_on(sway.display());
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), Some(px.path()));
    wait_on_outputs(&sh);
    px.hold();
    sh.ipc("now");
    sh.wait("the shell asking when the output shows a frame", |s| {
        s.lines()
            .any(|l| l.contains("-> wp_presentation#") && l.contains(".feedback("))
    });
    sh.ipc("lock");
    let why = dropped("the session is locked");
    sh.wait("the picker's work dropped", |s| s.contains(&why));
    sh.wait("the probe destroyed", |s| probes(s) == (1, 0));
    px.release();
    assert_picker_works_once_more(&sh, bin.path(), &started);
}

/// The output the closed panel was on is powered off: it shows no frame until it is switched
/// on again, and hyprpicker would start then. The work is dropped, with a line in the log.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_does_not_start_for_a_powered_off_output() {
    if !testenv::require_all(&["sway", "swaymsg", "mkfifo", "grim"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let sock = sway.ipc_socket();
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), Some(sway.display()));
    wait_on_outputs(&sh);
    swaymsg(&sock, &["output", SCREEN, "power", "off"]);
    sh.ipc("now");
    let why = dropped(&format!("its output {SCREEN} is powered off"));
    sh.wait("the picker's work dropped", |s| s.contains(&why));
    sh.wait("the probe destroyed", |s| probes(s) == (1, 0));
    swaymsg(&sock, &["output", SCREEN, "power", "on"]);
    assert_picker_works_once_more(&sh, bin.path(), &started);
}

/// The output is powered off while the picker waits for its frame there.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn powering_the_output_off_ends_the_wait_for_a_frame() {
    if !testenv::require_all(&["sway", "swaymsg", "mkfifo", "grim"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let sock = sway.ipc_socket();
    let px = testenv::HeldFeedback::start_on(sway.display());
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start(bin.path(), Some(px.path()));
    wait_on_outputs(&sh);
    px.hold();
    sh.ipc("now");
    sh.wait("the shell asking when the output shows a frame", |s| {
        s.lines()
            .any(|l| l.contains("-> wp_presentation#") && l.contains(".feedback("))
    });
    swaymsg(&sock, &["output", SCREEN, "power", "off"]);
    let why = dropped(&format!("its output {SCREEN} is powered off"));
    sh.wait("the picker's work dropped", |s| s.contains(&why));
    sh.wait("the probe destroyed", |s| probes(s) == (1, 0));
    px.release();
    swaymsg(&sock, &["output", SCREEN, "power", "on"]);
    assert_picker_works_once_more(&sh, bin.path(), &started);
}

/// With the session unlocked and the output on again, the panel is shown and the picker
/// asked for once more: hyprpicker starts for that, and it is its only start. One for the
/// dropped work would have come first: what it waited for is over before the panel is shown.
fn assert_picker_works_once_more(
    sh: &Shell,
    bin: &Path,
    started: &std::sync::mpsc::Receiver<String>,
) {
    let before = entered(&sh.log.text());
    sh.ipc("unlock");
    sh.ipc("show");
    sh.wait("the panel on the output again", |s| entered(s) > before);
    sh.ipc("now");
    assert_starts_off_screen(bin, started);
    assert_eq!(
        starts(bin),
        1,
        "hyprpicker started for the dropped work too"
    );
}

/// What a test needs of Hyprland beside its Wayland socket: the request socket, answering for
/// its animation settings, and the event socket, saying what the test tells it to.
struct FakeHypr {
    signature: String,
    dir: PathBuf,
    /// The replies to `j/animations` and `j/getoption animations:enabled`.
    settings: Arc<std::sync::Mutex<(String, String)>>,
    asked: Arc<std::sync::Mutex<Vec<String>>>,
    listeners: Arc<std::sync::Mutex<Vec<std::os::unix::net::UnixStream>>>,
}

/// `hyprctl -j animations` of Hyprland 0.57.1 with `layersOut` and `fadeLayersOut` set to
/// these speeds (a speed is a tenth of a second) and the other nodes as it prints them when
/// only `global` is set.
fn animations(layers_out: f64, fade_layers_out: f64) -> String {
    let node = |name: &str, speed: Option<f64>| {
        format!(
            r#"{{"name":"{name}","overridden":{},"bezier":"default","enabled":true,"speed":{:.2},"style":""}}"#,
            speed.is_some(),
            speed.unwrap_or(0.0)
        )
    };
    let nodes = [
        node("fade", None),
        node("global", Some(8.0)),
        node("layers", None),
        node("layersOut", Some(layers_out)),
        node("fadeLayers", None),
        node("fadeLayersOut", Some(fade_layers_out)),
    ];
    format!("[[{}],[]]", nodes.join(","))
}

const ANIMATIONS_ON: &str = r#"{"option": "animations:enabled", "bool": true, "set": true}"#;
const ANIMATIONS_OFF: &str = r#"{"option": "animations:enabled", "bool": false, "set": true}"#;

impl FakeHypr {
    /// An instance with these replies; without an event socket when not `events`.
    fn start(animations: &str, enabled: &str, events: bool) -> Self {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixListener;
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let signature = format!("ri-test-{}-{n}", std::process::id());
        let rt = std::env::var_os("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR");
        let dir = Path::new(&rt).join("hypr").join(&signature);
        std::fs::create_dir_all(&dir).unwrap();
        let settings = Arc::new(std::sync::Mutex::new((
            animations.to_string(),
            enabled.to_string(),
        )));
        let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
        let listeners = Arc::new(std::sync::Mutex::new(Vec::new()));
        let requests = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let (st, a) = (settings.clone(), asked.clone());
        std::thread::spawn(move || {
            for mut c in requests.incoming().flatten() {
                let mut buf = [0u8; 1024];
                let n = c.read(&mut buf).unwrap_or(0);
                let cmd = String::from_utf8_lossy(&buf[..n]).into_owned();
                let (animations, enabled) = st.lock().unwrap().clone();
                let reply = match cmd.as_str() {
                    "j/animations" => animations,
                    "j/getoption animations:enabled" => enabled,
                    _ => "unknown request".to_string(),
                };
                a.lock().unwrap().push(cmd);
                let _ = c.write_all(reply.as_bytes());
            }
        });
        if events {
            let events = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
            let l = listeners.clone();
            std::thread::spawn(move || {
                for c in events.incoming().flatten() {
                    l.lock().unwrap().push(c);
                }
            });
        }
        FakeHypr {
            signature,
            dir,
            settings,
            asked,
            listeners,
        }
    }

    /// Writes the line `event` to every listener of the event socket.
    fn say(&self, event: &str) {
        say(&self.listeners, event);
    }

    /// How many times `cmd` was asked.
    fn asked(&self, cmd: &str) -> usize {
        let asked = self.asked.lock().unwrap();
        asked.iter().filter(|c| *c == cmd).count()
    }
}

type Listeners = std::sync::Mutex<Vec<std::os::unix::net::UnixStream>>;

fn say(listeners: &Listeners, event: &str) {
    use std::io::Write;
    for mut c in listeners.lock().unwrap().iter() {
        c.write_all(format!("{event}\n").as_bytes()).unwrap();
    }
}

fn hang_up(listeners: &Listeners) {
    for c in listeners.lock().unwrap().drain(..) {
        let _ = c.shutdown(std::net::Shutdown::Both);
    }
}

impl Drop for FakeHypr {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The namespace of the layer the shell maps to hear of a frame.
const PROBE: &str = "impulse-shell-shown";

/// The shell has Hyprland's answer about its animations: a closed layer is animated `ms`.
fn wait_close_ms(sh: &Shell, ms: u32) {
    let line = format!("hyprland animates a closed layer {ms} ms");
    sh.wait("Hyprland's animation settings read", |s| s.contains(&line));
}

fn now_ns() -> u128 {
    let now = std::time::SystemTime::now();
    now.duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

/// When the fake hyprpicker in `bin` last started, by the clock of [`now_ns`].
fn started_ns(bin: &Path) -> u128 {
    let s = std::fs::read_to_string(bin.join("starts")).unwrap();
    s.lines().last().unwrap().parse().unwrap()
}

/// A proxy to the compositor that, like Hyprland, says `closelayer` on the event socket of
/// `hypr` when the shell destroys a layer, before the compositor answers what the shell asks
/// after; and the time it last said so of a window ([`now_ns`]).
fn announcing(hypr: &FakeHypr) -> (testenv::HeldFeedback, Arc<std::sync::Mutex<u128>>) {
    let px = testenv::HeldFeedback::start();
    let said = Arc::new(std::sync::Mutex::new(0));
    let (l, at) = (hypr.listeners.clone(), said.clone());
    px.on_layer_destroy(move |namespace| {
        say(&l, &format!("closelayer>>{namespace}"));
        if namespace != PROBE {
            *at.lock().unwrap() = now_ns();
        }
    });
    (px, said)
}

/// hyprpicker started no sooner than `ms` after `since`.
fn assert_started_after(bin: &Path, since: u128, ms: u128) {
    let waited = started_ns(bin).saturating_sub(since) / 1_000_000;
    assert!(
        waited >= ms,
        "hyprpicker started {waited} ms after Hyprland began to animate the panel away, \
         which takes {ms} ms"
    );
}

/// On Hyprland a closed panel leaves the screen by Hyprland's own animation, which starts as
/// it says `closelayer` and runs the longer of `layersOut` and `fadeLayersOut`: here 300 and
/// 500 ms. hyprpicker starts after that, each time, and the settings are asked for once.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_after_hyprlands_close_animation() {
    if !sway() {
        return;
    }
    let hypr = FakeHypr::start(&animations(3.0, 5.0), ANIMATIONS_ON, true);
    let (px, said) = announcing(&hypr);
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start_under(bin.path(), px.path(), &hypr);
    wait_on_outputs(&sh);
    wait_close_ms(&sh, 500);
    // What is not about a layer the shell closes changes nothing.
    hypr.say("closelayer>>someone-elses");
    hypr.say("no event at all");
    sh.ipc("now");
    assert_starts_off_screen(bin.path(), &started);
    assert_started_after(bin.path(), *said.lock().unwrap(), 500);
    let before = entered(&sh.log.text());
    sh.ipc("show");
    sh.wait("the panel on the output again", |s| entered(s) > before);
    sh.ipc("now");
    assert_starts_off_screen(bin.path(), &started);
    assert_started_after(bin.path(), *said.lock().unwrap(), 500);
    assert_eq!(starts(bin.path()), 2);
    assert_eq!(hypr.asked("j/animations"), 1, "settings read per pick");
    assert_eq!(hypr.asked("j/getoption animations:enabled"), 1);
}

/// With `animations:enabled` off Hyprland animates nothing, whatever the nodes say (here an
/// hour): there is nothing to wait out.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_does_not_wait_with_hyprlands_animations_off() {
    if !sway() {
        return;
    }
    let hypr = FakeHypr::start(&animations(36000.0, 36000.0), ANIMATIONS_OFF, true);
    let (px, _said) = announcing(&hypr);
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start_under(bin.path(), px.path(), &hypr);
    wait_on_outputs(&sh);
    wait_close_ms(&sh, 0);
    sh.ipc("now");
    assert_starts_off_screen(bin.path(), &started);
}

/// `configreloaded` is when the settings are read again: an hour of animation before it,
/// 500 ms after.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_reload_of_hyprlands_config_reads_its_animations_again() {
    if !sway() {
        return;
    }
    let hypr = FakeHypr::start(&animations(36000.0, 36000.0), ANIMATIONS_ON, true);
    let (px, said) = announcing(&hypr);
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start_under(bin.path(), px.path(), &hypr);
    wait_on_outputs(&sh);
    wait_close_ms(&sh, 3_600_000);
    hypr.settings.lock().unwrap().0 = animations(5.0, 3.0);
    hypr.say("configreloaded>>");
    wait_close_ms(&sh, 500);
    sh.ipc("now");
    assert_starts_off_screen(bin.path(), &started);
    assert_started_after(bin.path(), *said.lock().unwrap(), 500);
    assert_eq!(hypr.asked("j/animations"), 2);
}

/// Hyprland says `closelayer` for a layer it has mapped before it answers what the shell asks
/// after closing it. One it says nothing about by then it does not animate (here for an
/// hour): the wait does not go on for an event that is not coming.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_for_a_layer_hyprland_does_not_say_it_closed() {
    if !sway() {
        return;
    }
    let hypr = FakeHypr::start(&animations(36000.0, 36000.0), ANIMATIONS_ON, true);
    let px = testenv::HeldFeedback::start();
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start_under(bin.path(), px.path(), &hypr);
    wait_on_outputs(&sh);
    wait_close_ms(&sh, 3_600_000);
    sh.ipc("now");
    assert_starts_off_screen(bin.path(), &started);
}

/// Hyprland drops a listener of its event socket it cannot write to. A panel closed as that
/// happens is animated from then at the latest; one closed later gives an empty last frame,
/// as where there is no Hyprland, and the log says so.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprpicker_starts_when_hyprlands_event_socket_is_lost() {
    if !sway() {
        return;
    }
    let hypr = FakeHypr::start(&animations(3.0, 5.0), ANIMATIONS_ON, true);
    let px = testenv::HeldFeedback::start();
    let lost = Arc::new(std::sync::Mutex::new(0));
    let (l, at) = (hypr.listeners.clone(), lost.clone());
    px.on_layer_destroy(move |namespace| {
        hang_up(&l);
        if namespace != PROBE {
            *at.lock().unwrap() = now_ns();
        }
    });
    let (bin, started) = fake_hyprpicker(SCREEN);
    let sh = Shell::start_under(bin.path(), px.path(), &hypr);
    wait_on_outputs(&sh);
    wait_close_ms(&sh, 500);
    sh.ipc("now");
    assert_starts_off_screen(bin.path(), &started);
    assert_started_after(bin.path(), *lost.lock().unwrap(), 500);
    let said = "off screen: lost Hyprland's event socket";
    assert!(sh.log.text().contains(said), "no word of the lost socket");
    let before = entered(&sh.log.text());
    sh.ipc("show");
    sh.wait("the panel on the output again", |s| entered(s) > before);
    px.keep_surfaces_shown();
    sh.ipc("now");
    assert_starts_off_screen(bin.path(), &started);
}

/// A Hyprland whose event socket or animation settings the shell cannot have is treated as
/// no Hyprland: the closed panel gives an empty last frame (the compositor here keeps
/// showing its surface), and the log says why.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_closed_panel_gives_an_empty_frame_to_a_hyprland_that_does_not_answer() {
    if !sway() {
        return;
    }
    let cases = [
        (
            FakeHypr::start(&animations(36000.0, 36000.0), ANIMATIONS_ON, false),
            "off screen: no Hyprland event socket",
        ),
        (
            FakeHypr::start("unknown request", ANIMATIONS_ON, true),
            "off screen: cannot read Hyprland's animations",
        ),
        (
            FakeHypr::start(&animations(36000.0, 36000.0), "no such option", true),
            "off screen: cannot read Hyprland's animations",
        ),
    ];
    for (hypr, said) in cases {
        let px = testenv::HeldFeedback::start();
        let (bin, started) = fake_hyprpicker(SCREEN);
        let sh = Shell::start_under(bin.path(), px.path(), &hypr);
        wait_on_outputs(&sh);
        sh.wait(said, |s| s.contains(said));
        px.keep_surfaces_shown();
        sh.ipc("now");
        assert_starts_off_screen(bin.path(), &started);
    }
}
