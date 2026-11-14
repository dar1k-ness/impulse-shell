//! `Impulse/Screenshot`, `Impulse/ScreenRecorder`, `Impulse/ColorPicker` and
//! `Impulse/MangoHud`: the singletons the original's QML calls for screen captures, recordings, the
//! colour picker and the FPS limit (`qmltypes/Impulse/screenshot.qmltypes`).
//!
//! The work itself is [`crate::ports::shot`], [`crate::ports::screen`] and
//! [`crate::ports::record`]: wlr-screencopy instead of `grim`, `image` instead of `magick`, the
//! Wayland clipboard instead of `wl-copy`, `wf-recorder` started directly instead of through
//! `record.sh`. Every call returns at once; the work runs on a pool thread and a result comes
//! back as a signal, emitted on the main thread ([`crate::impulse::replies`]).
//!
//! Under a [`crate::native::SpawnHook`] (a sandboxed check) the hook is told the call (the
//! type, the method and its arguments; `hyprpicker -a` for the picker, which is a program the
//! call starts) and nothing touches the screen, the clipboard or the disk.

use crate::eval::{Interp, R};
use crate::impulse::{Replies, Type};
use crate::ports::{Io, Job, record, screen, shot};
use crate::runtime::World;
use crate::value::Value;

pub static SCREENSHOT: Type = Type {
    name: "Screenshot",
    methods: &[
        ("capture", capture),
        ("discardCaptures", discard_captures),
        ("copy", copy),
        ("edit", edit),
        ("search", search),
        ("recognizeText", recognize_text),
        ("copyScreen", copy_screen),
        ("measureLightness", measure_lightness),
    ],
    init: None,
};

pub static SCREEN_RECORDER: Type = Type {
    name: "ScreenRecorder",
    methods: &[
        ("check", check),
        ("toggle", toggle),
        ("toggleRegion", toggle_region),
        ("toggleFullscreen", toggle_fullscreen),
    ],
    init: None,
};

pub static COLOR_PICKER: Type = Type {
    name: "ColorPicker",
    methods: &[("pick", pick), ("pickOnceHidden", pick_once_hidden)],
    init: None,
};

pub static MANGO_HUD: Type = Type {
    name: "MangoHud",
    methods: &[("setFpsLimit", set_fps_limit)],
    init: None,
};

fn text(args: &[Value], i: usize) -> String {
    match args.get(i) {
        None | Some(Value::Undefined | Value::Null) => String::new(),
        Some(v) => v.to_js_string(),
    }
}

/// The call `ty.method(args)` goes to the sandbox's hook: `Some(result)` then, and the call
/// does nothing else.
fn hooked(ty: &Type, method: &str, args: &[Value]) -> Option<Option<String>> {
    let args: Vec<String> = args.iter().map(Value::to_js_string).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::native::hooked_call(ty.name, method, &args)
}

/// The colour picker's command line.
fn hyprpicker() -> Vec<String> {
    vec!["hyprpicker".into(), "-a".into()]
}

/// The way back to the main thread for the thread a call starts.
fn replies(it: &mut Interp) -> Option<Replies> {
    let w = crate::runtime::world_of(it)?;
    Some(crate::impulse::replies(&mut w.borrow_mut()))
}

/// What ends a capture's wait for the compositor: set when the World is dropped.
fn world_stop(it: &mut Interp) -> std::sync::Arc<crate::svc::Stop> {
    crate::runtime::world_of(it)
        .map(|w| w.borrow().jobs_stop.0.clone())
        .unwrap_or_default()
}

fn emit(w: &mut World, singleton: &str, signal: &str, params: &[&str], args: Vec<Value>) {
    if let Some(&item) = w.singletons.get(singleton) {
        crate::native::emit(w, item, signal, params, args);
    }
}

/// `Screenshot.captured(path, ok)`.
fn captured(back: &Replies, path: String, ok: bool) {
    back.send(move |w| {
        let args = vec![Value::str(&path), Value::Bool(ok)];
        emit(w, "Screenshot", "captured", &["path", "ok"], args);
    });
}

/// `Screenshot.lightnessMeasured(output, lightness)` for what the probe printed; nothing when
/// it failed or printed no number.
fn lightness_measured(back: &Replies, output: String, printed: Option<String>) {
    let lightness = printed.map_or(f64::NAN, |p| crate::builtins::parse_float(&p));
    if !lightness.is_finite() {
        return;
    }
    back.send(move |w| {
        let args = vec![Value::str(&output), Value::Num(lightness)];
        emit(
            w,
            "Screenshot",
            "lightnessMeasured",
            &["output", "lightness"],
            args,
        );
    });
}

/// `ScreenRecorder.checked(recording)`.
fn checked(back: &Replies, recording: bool) {
    back.send(move |w| {
        let args = vec![Value::Bool(recording)];
        emit(w, "ScreenRecorder", "checked", &["recording"], args);
    });
}

/// `capture(output, path)`: the output's frame as a PNG at `path`, then `captured(path, ok)`.
fn capture(it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let (output, path) = (text(args, 0), text(args, 1));
    let Some(back) = replies(it) else {
        return Ok(Value::Undefined);
    };
    if let Some(out) = hooked(&SCREENSHOT, "capture", args) {
        captured(&back, path, out.is_some());
        return Ok(Value::Undefined);
    }
    let dir = std::path::Path::new(&path)
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    shot::temp_started(&path);
    let job = Job::Shot(shot::Job::Temp {
        dir,
        output,
        path: path.clone(),
    });
    let io = Io {
        tx: None,
        stop: world_stop(it),
    };
    workpool::long(move || {
        let code = crate::jobs::caught(|| job.run(&io));
        captured(&back, path, code == 0);
    });
    Ok(Value::Undefined)
}

/// `discardCaptures()`: every capture no crop took over is removed (a selector closed without
/// a snip), also one still being written.
fn discard_captures(_it: &mut Interp, _this: &Value, _args: &[Value]) -> R<Value> {
    if !crate::native::sandboxed() {
        shot::temps_discard();
    }
    Ok(Value::Undefined)
}

/// The crop of the capture at `path` (its first argument, then x, y, width, height), handed to
/// `after` for the method `method`. The capture file is removed when the job ends.
fn crop(args: &[Value], method: &str, after: shot::After) {
    let path = text(args, 0);
    let n = |i: usize| args.get(i).map_or(0.0, |v| v.to_number()).round() as i64;
    let rect = (n(1), n(2), n(3), n(4));
    if hooked(&SCREENSHOT, method, args).is_some() {
        return;
    }
    shot::temp_claimed(&path);
    Job::Shot(shot::Job::Crop { path, rect, after }).start(None);
}

/// `copy(path, x, y, width, height, saveDir)`: to the clipboard, and saved as
/// `<saveDir>/screenshot-<date>.png` when `saveDir` is not empty.
fn copy(_it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let dir = text(args, 5);
    crop(
        args,
        "copy",
        shot::After::Copy((!dir.is_empty()).then_some(dir)),
    );
    Ok(Value::Undefined)
}

/// `edit(path, x, y, width, height, tool)`: piped into `<tool> -f -` (swappy, satty).
fn edit(_it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let tool = text(args, 5);
    crop(args, "edit", shot::After::Edit(tool));
    Ok(Value::Undefined)
}

/// `search(path, x, y, width, height, uploadEndpoint, searchBaseUrl)`: uploaded, then
/// `<searchBaseUrl><uploaded url>` is opened.
fn search(_it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let (endpoint, base) = (text(args, 5), text(args, 6));
    crop(args, "search", shot::After::Search { endpoint, base });
    Ok(Value::Undefined)
}

/// `recognizeText(path, x, y, width, height)`: `tesseract`'s text to the clipboard.
fn recognize_text(_it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    crop(args, "recognizeText", shot::After::Ocr);
    Ok(Value::Undefined)
}

/// `copyScreen()`: every output in one image, to the clipboard.
fn copy_screen(it: &mut Interp, _this: &Value, _args: &[Value]) -> R<Value> {
    if hooked(&SCREENSHOT, "copyScreen", &[]).is_none() {
        Job::Shot(shot::Job::FullCopy).start_under(None, world_stop(it));
    }
    Ok(Value::Undefined)
}

/// `measureLightness(output)`: the mean lightness of the output's frame, 0 to 100, as
/// `lightnessMeasured(output, lightness)`. A failed capture reports nothing.
fn measure_lightness(it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let output = text(args, 0);
    let Some(back) = replies(it) else {
        return Ok(Value::Undefined);
    };
    if let Some(out) = hooked(&SCREENSHOT, "measureLightness", args) {
        lightness_measured(&back, output, out);
        return Ok(Value::Undefined);
    }
    let stop = world_stop(it);
    workpool::long(move || {
        let printed = screen::lightness_of(&output, &stop);
        lightness_measured(&back, output, printed);
    });
    Ok(Value::Undefined)
}

/// `check()`: whether a `wf-recorder` runs (`pidof wf-recorder`), as `checked(recording)`.
fn check(it: &mut Interp, _this: &Value, _args: &[Value]) -> R<Value> {
    let Some(back) = replies(it) else {
        return Ok(Value::Undefined);
    };
    if let Some(out) = hooked(&SCREEN_RECORDER, "check", &[]) {
        checked(&back, out.is_some());
        return Ok(Value::Undefined);
    }
    workpool::run(move || checked(&back, !crate::osutil::pidof("wf-recorder").is_empty()));
    Ok(Value::Undefined)
}

/// Stops the running recording, or starts one as `a` says.
fn record(method: &str, args: &[Value], a: record::Args) {
    if hooked(&SCREEN_RECORDER, method, args).is_none() {
        Job::Record(a).start(None);
    }
}

/// `toggle(savePath)`: stops the running recording, or records a region picked with `slurp`
/// into the directory `savePath`.
fn toggle(_it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    record(
        "toggle",
        args,
        record::Args {
            save_path: text(args, 0),
            ..Default::default()
        },
    );
    Ok(Value::Undefined)
}

/// `toggleRegion(region, sound, savePath)`: the same for `region` (`"x,y WxH"`, wf-recorder's
/// `--geometry`), with the default sink's sound when `sound` is true and without sound otherwise.
fn toggle_region(_it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let region = text(args, 0);
    let sound = args.get(1).is_some_and(|v| v.truthy());
    record(
        "toggleRegion",
        args,
        record::Args {
            region: Some(region.clone()),
            silent: !sound,
            save_path: text(args, 2),
            ..Default::default()
        },
    );
    Ok(Value::Undefined)
}

/// `toggleFullscreen(savePath)`: the same for the focused monitor.
fn toggle_fullscreen(_it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    record(
        "toggleFullscreen",
        args,
        record::Args {
            fullscreen: true,
            save_path: text(args, 0),
            ..Default::default()
        },
    );
    Ok(Value::Undefined)
}

/// `pick()`: starts `hyprpicker -a`, which copies the colour the user clicks.
fn pick(_it: &mut Interp, _this: &Value, _args: &[Value]) -> R<Value> {
    if crate::native::hooked_spawn(&hyprpicker()).is_none() {
        crate::native::detach_program(&hyprpicker());
    }
    Ok(Value::Undefined)
}

/// `pickOnceHidden(window)`: `hyprpicker -a` once `window` (or the window of that item) is
/// off screen. hyprpicker
/// freezes the screen as it starts, so the window must be gone by then: it starts when the
/// host has seen a frame without the window presented ([`World::off_screen`]), and at once
/// with no frame-pacing host (no window is drawn).
fn pick_once_hidden(it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    if crate::native::hooked_spawn(&hyprpicker()).is_some() {
        return Ok(Value::Undefined);
    }
    let start: Box<dyn FnOnce() + Send> = Box::new(|| {
        crate::native::detach_program(&hyprpicker());
    });
    let win = match (args.first(), crate::runtime::world_of(it)) {
        (Some(Value::Item(i)), Some(w)) if w.borrow().frame_paced => {
            let win = crate::runtime::window_of(&w.borrow(), *i);
            win.map(|win| (w, win))
        }
        _ => None,
    };
    match win {
        Some((w, win)) => w.borrow_mut().off_screen.push((win, start)),
        None => start(),
    }
    Ok(Value::Undefined)
}

/// `setFpsLimit(fps)`: `fps_limit=<fps>` in `~/.config/MangoHud/MangoHud.conf`, then running
/// MangoHuds reload it (SIGUSR2). `false` when the file could not be written.
fn set_fps_limit(_it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let value = Value::Num(args.first().map_or(f64::NAN, |v| v.to_number())).to_js_string();
    if hooked(&MANGO_HUD, "setFpsLimit", args).is_some() {
        return Ok(Value::Bool(true));
    }
    if screen::set_fps_limit(&screen::mangohud_config(), &value).is_err() {
        return Ok(Value::Bool(false));
    }
    Job::Screen(screen::Job::FpsReload).start(None);
    Ok(Value::Bool(true))
}

#[cfg(test)]
mod tests {
    use crate::runtime::Runtime;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// The World's end stops the captures its calls started: `capture`, `measureLightness`
    /// and `copyScreen` each wait for the compositor's frame on a thread, and a compositor
    /// that never answers used to hold those threads (and their connections) for good.
    #[test]
    fn the_worlds_end_stops_captures_the_compositor_never_answers() {
        use std::io::Read as _;
        const TEST: &str = concat!(
            module_path!(),
            "::the_worlds_end_stops_captures_the_compositor_never_answers"
        );
        if !crate::testenv::in_own_process(TEST) {
            let dir = tempfile::tempdir().unwrap();
            crate::testenv::run_in_own_process_env(
                TEST,
                crate::ports::tests::mute_compositor_var(dir.path()),
                &["WAYLAND_SOCKET"],
            );
            return;
        }
        let mute = crate::ports::tests::mute_compositor();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image-DP-1");
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(&format!(
                "import QtQuick\nimport Impulse\nItem {{ function go() {{\n\
                 Screenshot.capture('DP-1', {path:?}); Screenshot.measureLightness('DP-1'); Screenshot.copyScreen() }} }}"
            ))
            .unwrap();
        rt.settle();
        let r = rt.call_method(root, "go", vec![]);
        assert!(matches!(r, Some(Ok(_))), "{r:?}");
        // Each of the three connected and sent its first requests: it waits for their answers.
        let clients: Vec<_> = (0..3)
            .map(|_| {
                let (mut c, _) = mute.accept().unwrap();
                assert!(c.read(&mut [0; 64]).unwrap() > 0);
                c
            })
            .collect();
        drop(rt);
        let (tx, idle) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            workpool::wait_idle();
            let _ = tx.send(());
        });
        // A capture that still waited for the compositor would keep the pool busy.
        idle.recv().unwrap();
        // The connections were open until here: only the stop can have ended the waits.
        drop(clients);
    }

    const DOC: &str = r#"import QtQuick
import Impulse
Item {
    id: root
    property var seen: []
    property var limited
    function note(s) { root.seen = root.seen.concat([s]) }
    Connections {
        target: Screenshot
        function onCaptured(path, ok) { root.note(`captured ${path} ${ok}`) }
        function onLightnessMeasured(output, lightness) { root.note(`lightness ${output} ${lightness}`) }
    }
    Connections {
        target: ScreenRecorder
        function onChecked(recording) { root.note(`checked ${recording}`) }
    }
    function go() {
        Screenshot.capture("DP-1", "/t/shot/image-DP-1");
        Screenshot.measureLightness("DP-1");
        ScreenRecorder.check();
        Screenshot.copy("/t/shot/image-DP-1", 1.4, 2.6, 30, 40, "");
        Screenshot.edit("/t/shot/image-DP-1", 1, 2, 3, 4, "satty");
        Screenshot.search("/t/shot/image-DP-1", 1, 2, 3, 4, "http://up", "http://lens?u=");
        Screenshot.recognizeText("/t/shot/image-DP-1", 1, 2, 3, 4);
        Screenshot.copyScreen();
        Screenshot.discardCaptures();
        ScreenRecorder.toggle();
        ScreenRecorder.toggleRegion("1,2 3x4");
        ScreenRecorder.toggleFullscreen();
        ColorPicker.pickOnceHidden(root);
        ColorPicker.pick();
        root.limited = MangoHud.setFpsLimit(60);
    }
}
"#;

    /// Runs `go()` with every command answered by `answer`; the notes and the commands.
    fn run(tag: &str, answer: Option<&'static str>) -> (String, String, Vec<String>) {
        let d = std::env::temp_dir().join(format!("ri-screenshot-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("Main.qml");
        std::fs::write(&f, DOC).unwrap();
        let log = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        crate::native::set_spawn_hook(Some(Box::new(move |a: &[String]| {
            l.borrow_mut().push(a.join(" "));
            answer.map(str::to_string)
        })));
        let mut rt = Runtime::new(None);
        let root = rt.load_file(&f).unwrap();
        rt.settle();
        let r = rt.call_method(root, "go", vec![]);
        assert!(matches!(r, Some(Ok(_))), "{r:?}");
        rt.poll_native();
        rt.settle();
        crate::native::set_spawn_hook(None);
        assert_eq!(rt.errors(), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&d);
        let seen = rt.get(root, "seen").to_js_string();
        let limited = rt.get(root, "limited").to_js_string();
        let log = log.borrow().clone();
        (seen, limited, log)
    }

    /// Every method is callable by name, tells the hook its call, and reports through its
    /// signal.
    #[test]
    fn methods_and_signals_by_name() {
        let (seen, limited, log) = run("ok", Some("42.5"));
        assert_eq!(
            seen,
            "captured /t/shot/image-DP-1 true,lightness DP-1 42.5,checked true"
        );
        assert_eq!(limited, "true");
        assert_eq!(
            log,
            [
                "Screenshot capture DP-1 /t/shot/image-DP-1",
                "Screenshot measureLightness DP-1",
                "ScreenRecorder check",
                "Screenshot copy /t/shot/image-DP-1 1.4 2.6 30 40 ",
                "Screenshot edit /t/shot/image-DP-1 1 2 3 4 satty",
                "Screenshot search /t/shot/image-DP-1 1 2 3 4 http://up http://lens?u=",
                "Screenshot recognizeText /t/shot/image-DP-1 1 2 3 4",
                "Screenshot copyScreen",
                "ScreenRecorder toggle",
                "ScreenRecorder toggleRegion 1,2 3x4",
                "ScreenRecorder toggleFullscreen",
                "hyprpicker -a",
                "hyprpicker -a",
                "MangoHud setFpsLimit 60",
            ]
        );
    }

    /// A failed capture says so, a failed probe reports no lightness, and a failed check
    /// means not recording.
    #[test]
    fn failures_are_reported() {
        let (seen, _, _) = run("fail", None);
        assert_eq!(seen, "captured /t/shot/image-DP-1 false,checked false");
    }
}
