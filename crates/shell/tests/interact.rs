//! Interaction equivalence: drives the same pointer/keyboard/wheel script (tests/interact/steps)
//! against `qs -p` (Qt) and `rusty-impulse -p` (native) on the harness in tests/interact/shell.qml, and
//! checks that every step fires the same signals/handlers with the same resulting state.
//!
//! Nothing waits on wall time: each step names the `IACT` lines it ends with (`=> line`), and the
//! next step starts once the shell printed them. Event timestamps run on a scripted clock, so
//! timestamp-based behaviour (double clicks, flick velocity) is the same on every run.
//!
//! Both shells and the virtual devices run in a sway of the test's own (`testenv::Sway`), not
//! in the shared one (WAYLAND_DISPLAY): tests of other binaries map windows there meanwhile
//! (install_rollback's shell), and a step that runs while one appears can stay without its
//! line.
//!
//! Needs `sway` (with `zwlr_virtual_pointer_v1` + `zwp_virtual_keyboard_v1`) and `qs` on PATH
//! (or RI_IACT_QS). The native binary is RI_IACT_NATIVE or this crate's `rusty-impulse`, run with
//! RI_INTERP=1 when built with `interp`. Fails without them (skips only with RI_EQUIV_SKIP=1).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};

use wayland_client::protocol::{wl_output, wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const STEPS: &str = include_str!("interact/steps");
/// Scripted time between two steps: longer than the double-click interval (400 ms), so two clicks
/// in successive steps never make a double click.
const STEP_GAP_MS: u32 = 500;
/// QSG_FIXED_ANIMATION_STEP's step: what each frame moves animations by.
const FRAME_MS: u32 = 16;

#[derive(Default)]
struct Globals {
    seat: Option<wl_seat::WlSeat>,
    output: Option<wl_output::WlOutput>,
    vptr: Option<ZwlrVirtualPointerManagerV1>,
    vkbd: Option<ZwpVirtualKeyboardManagerV1>,
    mode: (u32, u32),
}

impl Dispatch<wl_output::WlOutput, ()> for Globals {
    fn event(
        g: &mut Self,
        _: &wl_output::WlOutput,
        ev: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Mode {
            flags: wayland_client::WEnum::Value(f),
            width,
            height,
            ..
        } = ev
            && f.contains(wl_output::Mode::Current)
        {
            g.mode = (width as u32, height as u32);
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
    fn event(
        g: &mut Self,
        reg: &wl_registry::WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = ev
        {
            match interface.as_str() {
                "wl_seat" if g.seat.is_none() => g.seat = Some(reg.bind(name, 1, qh, ())),
                "wl_output" if g.output.is_none() => g.output = Some(reg.bind(name, 1, qh, ())),
                "zwlr_virtual_pointer_manager_v1" => {
                    g.vptr = Some(reg.bind(name, version.min(2), qh, ()))
                }
                "zwp_virtual_keyboard_manager_v1" => g.vkbd = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

delegate_noop!(Globals: ignore wl_seat::WlSeat);
delegate_noop!(Globals: ZwlrVirtualPointerManagerV1);
delegate_noop!(Globals: ZwlrVirtualPointerV1);
delegate_noop!(Globals: ZwpVirtualKeyboardManagerV1);
delegate_noop!(Globals: ZwpVirtualKeyboardV1);

/// Virtual pointer + keyboard on the compositor's first output (absolute motion in output pixels).
struct Input {
    conn: Connection,
    queue: EventQueue<Globals>,
    state: Globals,
    ptr: ZwlrVirtualPointerV1,
    kbd: ZwpVirtualKeyboardV1,
    /// Event timestamps advance only by scripted pauses, so timestamp-based behaviour
    /// (double clicks, flicks, wheel acceleration) doesn't depend on scheduling.
    clock: u32,
    /// The output of the shell receiving the input, whose frames pace it.
    pace: Option<Arc<(Mutex<Output>, Condvar)>>,
    /// Frames since the last event was sent.
    idle: u32,
    _keymap: std::fs::File,
}

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

impl Input {
    /// On the compositor listening on the socket `display`.
    fn connect(display: &Path) -> Result<Self, String> {
        let stream = std::os::unix::net::UnixStream::connect(display)
            .map_err(|e| format!("no wayland display {}: {e}", display.display()))?;
        let conn = Connection::from_socket(stream).map_err(|e| e.to_string())?;
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut state = Globals::default();
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        if state.mode.0 < 1200 || state.mode.1 < 760 {
            return Err(format!(
                "first output is {:?}, need at least 1200x760",
                state.mode
            ));
        }
        let (Some(vptr), Some(vkbd), Some(seat)) =
            (state.vptr.clone(), state.vkbd.clone(), state.seat.clone())
        else {
            return Err("compositor lacks zwlr_virtual_pointer_v1/zwp_virtual_keyboard_v1".into());
        };
        let ptr = match &state.output {
            Some(o) if vptr.version() >= 2 => {
                vptr.create_virtual_pointer_with_output(Some(&seat), Some(o), &qh, ())
            }
            _ => vptr.create_virtual_pointer(Some(&seat), &qh, ()),
        };
        let kbd = vkbd.create_virtual_keyboard(&seat, &qh, ());
        let ctx = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
        let km = xkbcommon::xkb::Keymap::new_from_names(
            &ctx,
            "",
            "",
            "us",
            "",
            None,
            xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or("xkb: cannot compile us keymap")?;
        let text = km.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
        let mut keymap = tempfile::tempfile().map_err(|e| e.to_string())?;
        keymap.write_all(text.as_bytes()).unwrap();
        keymap.write_all(&[0]).unwrap();
        kbd.keymap(1, keymap.as_fd(), text.len() as u32 + 1);
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        Ok(Self {
            conn,
            queue,
            state,
            ptr,
            kbd,
            clock: 0,
            pace: None,
            idle: 0,
            _keymap: keymap,
        })
    }

    fn t(&self) -> u32 {
        self.clock
    }

    /// Sends what was queued and, once a shell runs, lets it draw two frames (`frames`), so it
    /// handled that on its own before anything else is sent.
    fn flush(&mut self) {
        self.conn.flush().unwrap();
        self.queue.roundtrip(&mut self.state).unwrap();
        self.idle = 0;
        self.frames(2);
    }

    /// Lets the shell draw `n` frames. Under `QSG_FIXED_ANIMATION_STEP` each moves its animations
    /// `FRAME_MS`, and the scripted clock moves with them: event timestamps and animations
    /// advance alike, as for a user on a display that keeps up.
    fn frames(&mut self, n: u32) {
        if let Some(out) = &self.pace {
            frames(out, n);
        }
        self.clock += n * FRAME_MS;
        self.idle += n;
    }

    /// Lets about `ms` pass since the last event on the scripted and the animation clock (whole
    /// frames, counting those `flush` already waited).
    fn pause(&mut self, ms: u32) {
        self.frames(ms.div_ceil(FRAME_MS).saturating_sub(self.idle));
    }

    fn move_to(&mut self, x: i32, y: i32) {
        self.ptr.motion_absolute(
            self.t(),
            x as u32,
            y as u32,
            self.state.mode.0,
            self.state.mode.1,
        );
        self.ptr.frame();
        self.flush();
    }

    fn button(&mut self, b: u32, down: bool) {
        let st = if down {
            wl_pointer::ButtonState::Pressed
        } else {
            wl_pointer::ButtonState::Released
        };
        self.ptr.button(self.t(), b, st);
        self.ptr.frame();
        self.flush();
    }

    /// A click: press and, `ms` later on the scripted clock, release, sent together so the shell
    /// reads both at once and no frame of a loaded host comes between them. Qt still handles
    /// the release from a 0 ms timer, after the press: see `Shell::held`.
    fn tap(&mut self, b: u32, ms: u32) {
        self.ptr
            .button(self.t(), b, wl_pointer::ButtonState::Pressed);
        self.ptr.frame();
        self.clock += ms;
        self.ptr
            .button(self.t(), b, wl_pointer::ButtonState::Released);
        self.ptr.frame();
        self.flush();
    }

    fn wheel(&mut self, notches: i32) {
        for _ in 0..notches.abs() {
            let t = self.t();
            self.ptr.axis_source(wl_pointer::AxisSource::Wheel);
            self.ptr.axis(
                t,
                wl_pointer::Axis::VerticalScroll,
                15.0 * notches.signum() as f64,
            );
            self.ptr.axis_discrete(
                t,
                wl_pointer::Axis::VerticalScroll,
                15.0 * notches.signum() as f64,
                notches.signum(),
            );
            self.ptr.frame();
            self.flush();
            self.pause(60);
        }
    }

    /// One touchpad frame: a finger moved by `dx`, `dy` (wl_pointer.axis values, no clicks).
    fn swipe(&mut self, dx: f64, dy: f64) {
        let t = self.t();
        self.ptr.axis_source(wl_pointer::AxisSource::Finger);
        for (axis, v) in [
            (wl_pointer::Axis::HorizontalScroll, dx),
            (wl_pointer::Axis::VerticalScroll, dy),
        ] {
            if v != 0.0 {
                self.ptr.axis(t, axis, v);
            }
        }
        self.ptr.frame();
        self.flush();
        self.pause(60);
    }

    /// A key pressed and released 32 ms later on the scripted clock, sent as one batch: the
    /// shell takes both from the socket together, so it stops its repeat timer in the turn that
    /// started it. With frames waited for in between, a shell slowed down by load held the key
    /// past sway's 600 ms repeat delay, which clients count on the wall clock, and typed it again.
    fn key(&mut self, code: u32) {
        self.kbd.key(self.t(), code, 1);
        self.clock += 2 * FRAME_MS;
        self.kbd.key(self.t(), code, 0);
        self.flush();
        self.pause(30);
    }
}

/// evdev keycodes for the key names steps use.
fn keycode(name: &str) -> u32 {
    const LETTERS: &str = "qwertyuiop    asdfghjkl     zxcvbnm";
    match name {
        "Escape" => 1,
        "BackSpace" => 14,
        "Tab" => 15,
        "Return" => 28,
        "space" => 57,
        "Home" => 102,
        "Up" => 103,
        "Left" => 105,
        "Right" => 106,
        "End" => 107,
        "Down" => 108,
        c if c.len() == 1 => {
            let i = LETTERS
                .find(c)
                .unwrap_or_else(|| panic!("no keycode for {c:?}"));
            16 + i as u32
        }
        _ => panic!("no keycode for {name:?}"),
    }
}

/// Plays one step's input and returns once the shell printed the lines it waits for (`want`, in
/// order): those lines and everything before them are the step's trace. A `hold` keeps the button
/// down until the first of them arrived (pressAndHold fires on the shell's own timer).
fn run_step(inp: &mut Input, sh: &mut Shell, op: &str, a: &[&str], want: &[&str]) -> Vec<String> {
    let n = |i: usize| a[i].parse::<i32>().unwrap();
    let btn = |s: Option<&&str>| match s.copied() {
        Some("right") => BTN_RIGHT,
        Some("middle") => BTN_MIDDLE,
        _ => BTN_LEFT,
    };
    let mut want = want;
    match op {
        "move" => inp.move_to(n(0), n(1)),
        "click" | "dclick" => {
            inp.move_to(n(0), n(1));
            inp.pause(50);
            let b = btn(a.get(2));
            for _ in 0..if op == "dclick" { 2 } else { 1 } {
                inp.tap(b, 40);
                inp.pause(60);
            }
            // Qt's SpinBox took the click for a press-and-hold and stepped nothing (`IHELD`):
            // the click again, later than a double click of the first.
            while op == "click" && sh.held(want[0]) {
                inp.clock += STEP_GAP_MS;
                inp.tap(b, 40);
                inp.pause(60);
            }
        }
        "hold" => {
            inp.move_to(n(0), n(1));
            inp.pause(50);
            inp.button(BTN_LEFT, true);
            sh.wait_for(&want[..1]);
            want = &want[1..];
            inp.pause(n(2) as u32);
            inp.button(BTN_LEFT, false);
        }
        "drag" => {
            let (x0, y0, x1, y1) = (n(0), n(1), n(2), n(3));
            inp.move_to(x0, y0);
            inp.pause(50);
            inp.button(BTN_LEFT, true);
            for i in 1..=12 {
                inp.pause(25);
                inp.move_to(x0 + (x1 - x0) * i / 12, y0 + (y1 - y0) * i / 12);
            }
            inp.pause(60);
            // The lines before `release` come with the button down, and not at a frame the
            // script counts: the release waits for them.
            if let Some(k) = want.iter().position(|w| *w == "release") {
                sh.wait_for(&want[..k]);
                want = &want[k + 1..];
            }
            inp.button(BTN_LEFT, false);
        }
        "wheel" => {
            inp.move_to(n(0), n(1));
            inp.pause(50);
            inp.wheel(if a[2] == "up" { -n(3) } else { n(3) });
        }
        "swipe" => {
            inp.move_to(n(0), n(1));
            inp.pause(50);
            inp.swipe(a[2].parse().unwrap(), a[3].parse().unwrap());
        }
        "key" => inp.key(keycode(a[0])),
        "type" => {
            for c in a.join(" ").chars() {
                let s = c.to_string();
                inp.key(keycode(if c == ' ' { "space" } else { &s }));
            }
        }
        _ => panic!("unknown step op {op}"),
    }
    sh.wait_for(want);
    // Scripted time only: everything the step waited for has happened, so no animation is under
    // way to keep in step with the event timestamps.
    inp.clock += STEP_GAP_MS;
    sh.take()
}

/// What a shell printed so far: its `IACT` lines, all its output, and how many of its two
/// output pipes are still open.
#[derive(Default)]
struct Output {
    iact: Vec<String>,
    all: Vec<String>,
    /// Frames the shell drew (`ITICK` lines, not kept in `all`).
    ticks: u64,
    /// Clicks Qt's SpinBox did not step for (`IHELD` lines of the harness).
    held: u64,
    open: usize,
}

/// Blocks until the shell drew `n` more frames (`ITICK` lines). The first may have been under
/// way when the input sent before this arrived; the later ones began after the shell's event
/// loop handled that input.
fn frames(out: &(Mutex<Output>, Condvar), n: u32) {
    let (lock, cv) = out;
    let mut o = lock.lock().unwrap();
    let end = o.ticks + u64::from(n);
    while o.ticks < end {
        if o.open == 0 {
            panic!("shell exited:\n{}", o.all.join("\n"));
        }
        o = cv.wait(o).unwrap();
    }
}

/// A running shell whose `IACT` log lines are collected as they arrive.
struct Shell {
    child: Child,
    out: Arc<(Mutex<Output>, Condvar)>,
    /// `iact` lines already handed out by `take`.
    taken: usize,
    /// End (exclusive) of the lines `wait_for` matched: what the next `take` hands out.
    matched: usize,
    /// `IHELD` lines `held` answered for.
    held: u64,
}

impl Shell {
    fn spawn(mut cmd: Command, dir: &Path) -> Self {
        let mut child = cmd
            .arg("-p")
            .arg(dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn shell");
        let out = Arc::new((
            Mutex::new(Output {
                open: 2,
                ..Output::default()
            }),
            Condvar::new(),
        ));
        for pipe in [
            Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>,
            Box::new(child.stderr.take().unwrap()),
        ] {
            let out = out.clone();
            std::thread::spawn(move || {
                for l in BufReader::new(pipe).lines().map_while(Result::ok) {
                    let mut o = out.0.lock().unwrap();
                    if l.ends_with("ITICK") {
                        o.ticks += 1;
                    } else {
                        if l.ends_with("IHELD") {
                            o.held += 1;
                        }
                        if let Some(i) = l.find("IREPEATED ") {
                            let n = &l[i + 10..];
                            eprintln!("the spin box stepped {n} times for a held click: one kept");
                        }
                        if let Some(i) = l.find("IACT ") {
                            // Echoed, so a step stuck waiting shows what the shell did print.
                            eprintln!("  {}", l[i + 5..].trim_end());
                            o.iact.push(l[i + 5..].trim_end().to_string());
                        }
                        o.all.push(l);
                    }
                    drop(o);
                    out.1.notify_all();
                }
                out.0.lock().unwrap().open -= 1;
                out.1.notify_all();
            });
        }
        Self {
            child,
            out,
            taken: 0,
            matched: 0,
            held: 0,
        }
    }

    /// Blocks until the shell printed `want` in this order after the lines already matched (a
    /// line matches a `want` that is it or its leading words). No
    /// deadline of its own: a shell that never prints them leaves the test to nextest's
    /// slow-timeout (.config/nextest.toml); the shell exiting first fails the test.
    fn wait_for(&mut self, want: &[&str]) {
        eprintln!("waiting for {want:?}");
        let (lock, cv) = &*self.out;
        let mut o = lock.lock().unwrap();
        for w in want {
            loop {
                if let Some(i) = o.iact[self.matched..].iter().position(|l| {
                    l.strip_prefix(w)
                        .is_some_and(|r| r.is_empty() || r.starts_with(' '))
                }) {
                    self.matched += i + 1;
                    break;
                }
                if o.open == 0 {
                    panic!(
                        "shell exited without printing IACT {w}:\n{}",
                        o.all.join("\n")
                    );
                }
                o = cv.wait(o).unwrap();
            }
        }
    }

    /// Blocks until the shell printed `want` after the lines already matched (false; the line
    /// is left for `wait_for`) or the harness reported a click Qt's SpinBox did not step for
    /// (true): it prints `IHELD` at the release, where a step would have been printed before.
    fn held(&mut self, want: &str) -> bool {
        let (lock, cv) = &*self.out;
        let mut o = lock.lock().unwrap();
        loop {
            if o.held > self.held {
                self.held = o.held;
                eprintln!("the spin box took the click for a press-and-hold: clicking again");
                return true;
            }
            if o.iact[self.matched..].iter().any(|l| {
                l.strip_prefix(want)
                    .is_some_and(|r| r.is_empty() || r.starts_with(' '))
            }) {
                return false;
            }
            if o.open == 0 {
                panic!(
                    "shell exited without printing IACT {want}:\n{}",
                    o.all.join("\n")
                );
            }
            o = cv.wait(o).unwrap();
        }
    }

    /// The `IACT` lines up to the last one `wait_for` matched, not yet taken.
    fn take(&mut self) -> Vec<String> {
        let o = self.out.0.lock().unwrap();
        let got = o.iact[self.taken..self.matched].to_vec();
        self.taken = self.matched;
        got
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Step label -> the IACT lines it produced, in script order.
type Trace = Vec<(String, Vec<String>)>;

/// Runs the script against `cmd` in a sway of its own: the steps' traces, and every line the
/// shell printed while they ran.
fn trace(mut cmd: Command, dir: &Path) -> (Trace, Vec<String>) {
    let sway = testenv::Sway::start();
    sway.env(&mut cmd);
    let mut inp = Input::connect(sway.display()).expect("virtual input");
    // Park the pointer off every control so the first step's enter is deterministic.
    inp.move_to(1190, 750);
    let mut sh = Shell::spawn(cmd, dir);
    inp.pace = Some(sh.out.clone());
    // The harness prints `ready` once the original's Config loaded and the pointer entered its mapped
    // window: from then on input reaches it.
    sh.wait_for(&["ready"]);
    let mut out = vec![("startup".to_string(), sh.take())];
    for line in STEPS.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let (label, rest) = line
            .split_once(':')
            .expect("steps line is `label: op args => line...`");
        let mut parts = rest.split("=>").map(str::trim);
        let cmd = parts.next().unwrap();
        let want: Vec<&str> = parts.collect();
        assert!(
            !want.is_empty(),
            "step `{line}` names no IACT line to wait for"
        );
        let w: Vec<&str> = cmd.split_whitespace().collect();
        eprintln!("step {label}: {cmd}");
        let mut got = run_step(&mut inp, &mut sh, w[0], &w[1..], &want);
        // Event compression differs in how many motion events reach a frame, not in what fires.
        got.dedup();
        out.push((format!("{label}: {cmd}"), got));
    }
    let log = sh.out.0.lock().unwrap().all.clone();
    (out, log)
}

fn harness_dir() -> tempfile::TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    testenv::tree_in_quickshells_words(&root.join("qml"), dir.path());
    std::fs::copy(
        root.join("crates/shell/tests/interact/shell.qml"),
        dir.path().join("shell.qml"),
    )
    .unwrap();
    // The harness's spin box: a widget no window of the shell uses any more, kept with the tests.
    let spin = "modules/common/widgets/StyledSpinBox.qml";
    let text = std::fs::read_to_string(
        root.join("crates/qml/tests/fixtures/original-unshipped")
            .join(spin),
    )
    .unwrap();
    std::fs::write(dir.path().join(spin), testenv::in_quickshells_words(&text)).unwrap();
    dir
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .to_str()?
        .split(':')
        .map(|d| Path::new(d).join(bin))
        .find(|p| p.is_file())
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn controls_match_qt_fails_without_its_environment() {
    testenv::assert_fails_when_missing(
        "controls_match_qt",
        &[("PATH", Some("/nonexistent")), ("RI_IACT_QS", None)],
    );
}

/// A SwipeView dragged to its next page and released while nothing else on its window animates
/// (tests/interact/swipe.qml) comes to rest on that page: each frame that shows it between two
/// pages is one the view asked for itself. In shell.qml the same drag settles only while some
/// other animation of that window is still drawing frames.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn swipe_released_with_nothing_else_animating_comes_to_rest() {
    if !testenv::require("sway") {
        return;
    }
    let sway = testenv::Sway::start();
    let mut inp = Input::connect(sway.display()).expect("virtual input");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("shell.qml"),
        include_str!("interact/swipe.qml"),
    )
    .unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
    for (k, d) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        cmd.env(k, home.path().join(d));
    }
    cmd.env("RI_INTERP", "1")
        .env("QSG_FIXED_ANIMATION_STEP", "1");
    sway.env(&mut cmd);
    inp.move_to(1190, 750);
    let mut sh = Shell::spawn(cmd, dir.path());
    inp.pace = Some(sh.out.clone());
    sh.wait_for(&["ready"]);
    sh.take();
    let got = run_step(
        &mut inp,
        &mut sh,
        "drag",
        &["1120", "400", "990", "400"],
        &["swipe"],
    );
    assert_eq!(got, ["swipe rest 200"]);
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn controls_match_qt() {
    let skip = |why: String| testenv::unavailable(format!("interaction equivalence: {why}"));
    if !testenv::require("sway") {
        return;
    }
    let Some(qs) = std::env::var_os("RI_IACT_QS")
        .map(PathBuf::from)
        .or_else(|| which("qs"))
    else {
        return skip("no qs on PATH".into());
    };
    let native = std::env::var_os("RI_IACT_NATIVE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_rusty-impulse")));
    let dir = harness_dir();
    // The original's Config writes config.json under XDG_CONFIG_HOME: keep the user's untouched.
    let home = tempfile::tempdir().unwrap();
    // Directories.qml makes this directory through `Impulse.Files`, which does nothing in
    // Quickshell's copy of the module: without it Config never loads there.
    std::fs::create_dir_all(home.path().join("config/rusty-impulse")).unwrap();
    let isolated = |bin: &Path| {
        let mut c = Command::new(bin);
        for (k, d) in [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            c.env(k, home.path().join(d));
        }
        // Animations (flicks, popup transitions) move a fixed 16 ms per frame, so with input
        // paced by frames both shells see each event at the same point of every animation.
        c.env("QSG_FIXED_ANIMATION_STEP", "1");
        c
    };
    // The tree says `import Impulse`, which the shell implements itself: Quickshell gets the
    // same types with methods that do nothing.
    let modules = tempfile::tempdir().unwrap();
    testenv::impulse_qt_module(modules.path());
    let mut qcmd = isolated(&qs);
    qcmd.env("QML_IMPORT_PATH", modules.path());
    let (qt, qt_log) = trace(qcmd, dir.path());
    let mut ncmd = isolated(&native);
    ncmd.env("RI_INTERP", "1");
    let (rs, rs_log) = trace(ncmd, dir.path());

    for (name, t) in [("qt", &qt), ("native", &rs)] {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("interact-{name}.txt"));
        let text: String = t
            .iter()
            .map(|(l, v)| {
                format!(
                    "{l}\n{}",
                    v.iter().map(|x| format!("  {x}\n")).collect::<String>()
                )
            })
            .collect();
        std::fs::write(&path, text).unwrap();
        eprintln!("{name} trace: {}", path.display());
    }
    // A step must fire at least one handler under Qt, or the script is not exercising anything.
    let dead: Vec<_> = qt
        .iter()
        .filter(|(_, v)| v.is_empty())
        .map(|(l, _)| l.clone())
        .collect();
    let mut diffs = Vec::new();
    for ((label, q), (_, n)) in qt.iter().zip(&rs) {
        if q != n {
            diffs.push(format!("{label}\n  qt:     {q:?}\n  native: {n:?}"));
        }
    }
    let per_control: BTreeMap<&str, usize> = qt
        .iter()
        .filter_map(|(l, _)| l.split(':').next())
        .fold(BTreeMap::new(), |mut m, c| {
            *m.entry(c).or_default() += 1;
            m
        });
    eprintln!("interaction steps per control: {per_control:?}");
    assert!(
        dead.is_empty(),
        "steps that fire nothing under Qt (fix the script):\n{}",
        dead.join("\n")
    );
    // A write to a signal (`root.clicked = false`, which the original's GroupButton ended its long
    // press with) is a TypeError in the log of both shells.
    let refused: Vec<String> = [("qt", &qt_log), ("native", &rs_log)]
        .iter()
        .flat_map(|(name, log)| {
            let bad = log.iter().filter(|l| l.contains("read-only property"));
            bad.map(move |l| format!("{name}: {l}"))
        })
        .collect();
    assert!(
        refused.is_empty(),
        "writes the shells refused:\n{}",
        refused.join("\n")
    );
    assert!(
        diffs.is_empty(),
        "{} of {} steps differ from Qt:\n{}",
        diffs.len(),
        qt.len(),
        diffs.join("\n")
    );
}
