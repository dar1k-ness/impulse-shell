//! The `Impulse` QML module: the shell's own types, written in Rust and reached from QML by
//! name (`import Impulse`), the way Quickshell's QML reaches its C++ types.
//!
//! A type is declared in a `qmltypes/Impulse/*.qmltypes` file (its properties, signals and
//! methods) and gets its behaviour from the module that implements it: a service
//! (`isSingleton: true`) from one [`Type`] of [`TYPES`], an element QML creates from one
//! [`Element`] of [`ELEMENTS`]. See `docs/native-types.md`.
use crate::eval::NativeFn;
use crate::runtime::World;
use crate::value::Value;

/// The Rust side of a singleton the `Impulse` module declares.
pub struct Type {
    /// The exported QML name (`Clipboard` for `exports: ["Impulse/Clipboard 1.0"]`).
    pub name: &'static str,
    /// The declared methods, by name. One left out answers null and reports itself once
    /// (`native: no handler for effect call`).
    pub methods: &'static [(&'static str, NativeFn)],
    /// Runs once, when QML first names the singleton and its instance has the declared
    /// properties with their default values: sets what the properties start as.
    pub init: Option<fn(&mut World, usize)>,
}

/// The singletons of the module, one line per type, sorted by name.
pub const TYPES: &[&Type] = &[
    &crate::ports::clip::TYPE,
    &crate::screenshot::COLOR_PICKER,
    &crate::native::CROSSFADE_TYPE,
    &crate::brightness::TYPE,
    &crate::easyeffects::TYPE,
    &crate::emojis::TYPE,
    &crate::files::TYPE,
    &crate::fprint::TYPE,
    &crate::jslibs::fuzzysort::TYPE,
    &crate::ports::vkbd::TYPE,
    &crate::keyring::TYPE,
    &crate::jslibs::levendist::TYPE,
    &crate::screenshot::MANGO_HUD,
    &crate::shapes::MATERIAL_SHAPES,
    &crate::pw::MICROPHONE,
    &crate::shapes::MORPH,
    &crate::musicrecognizer::TYPE,
    &crate::network::TYPE,
    &crate::notifier::TYPE,
    &crate::processes::TYPE,
    &crate::shapes::ROUNDED_POLYGON,
    &crate::screenshot::SCREEN_RECORDER,
    &crate::screenshot::SCREENSHOT,
    &crate::session::TYPE,
    &crate::shellcontrol::TYPE,
    &crate::system::TYPE,
    &crate::systemsound::TYPE,
    #[cfg(test)]
    &tests::PROBE,
    &crate::warp::TYPE,
];

pub fn find(name: &str) -> Option<&'static Type> {
    TYPES.iter().copied().find(|t| t.name == name)
}

/// The function behind method `method` of singleton `name`.
pub fn method(name: &str, method: &str) -> Option<NativeFn> {
    find(name)?
        .methods
        .iter()
        .find(|(m, _)| *m == method)
        .map(|(_, f)| *f)
}

/// A method of an element: the instance, then the arguments.
pub type ElementFn = fn(&mut World, usize, &[Value]) -> Value;

/// The Rust side of an element the `Impulse` module declares and QML creates
/// (`HyprlandRequest { }`). `..Element::new(name)` fills the fields a type does not use.
pub struct Element {
    /// The exported QML name.
    pub name: &'static str,
    /// The declared methods, by name.
    pub methods: &'static [(&'static str, ElementFn)],
    /// The element is a `Process` (declared with `prototype: "Process"`) whose work is this
    /// instead of a command: `running: true` starts the job on a pool thread, what it writes
    /// arrives through `stdout` / `stderr`, and its return value is the `exited` code. The
    /// function runs on the main thread and reads the job's inputs from the instance's
    /// properties; `None` starts nothing and `running` goes back to false.
    pub job: Option<fn(&mut World, usize) -> Option<crate::jobs::Job>>,
    /// Runs when the element's `running` property is written once its component is complete,
    /// and at completion if it was created with `running: true`, as a C++ setter would.
    pub running: Option<fn(&mut World, usize, bool)>,
    /// Runs when an instance is created, before QML assigns its properties.
    pub created: Option<fn(&mut World, usize)>,
}

impl Element {
    pub const fn new(name: &'static str) -> Element {
        Element {
            name,
            methods: &[],
            job: None,
            running: None,
            created: None,
        }
    }
}

/// The elements of the module made through an [`Element`], one line per type, sorted by
/// name. Five more types of the module are built into the runtime instead: `DesktopContainer`
/// ([`crate::desktop_container`]), `PanelContainer` ([`crate::panel`]), `SidebarContainer`
/// and `WidgetHost` (`runtime.rs`), and `WifiAccessPoint`, which `Network` creates
/// ([`crate::network`]).
pub const ELEMENTS: &[&Element] = &[
    &crate::jobs_ai::translate::ELEMENT,
    &crate::cava::ELEMENT,
    &crate::imgjobs::DIRECTORY_THUMBNAILS,
    &crate::jobs::DOWNLOAD,
    &crate::jobs_lists::FILE_MIME_TYPE,
    &crate::procs::GEOCLUE_CHECK,
    &crate::jobs_wall_gcloud_trans::gcloud::TOKEN,
    &crate::jobs_wall_gcloud_trans::gcloud_api::TRANSLATE_ELEMENT,
    &crate::jobs_wall_gcloud_trans::gcloud_api::VISION_ELEMENT,
    &crate::hyprconf::FILE,
    &crate::hyprctl::REQUEST,
    &crate::hyprctl::SUNSET_CLIENT,
    &crate::jobs::IMAGE_DOWNLOAD,
    &crate::imgjobs::IMAGE_REGIONS,
    &crate::procs::IMAGE_SIZE,
    &crate::imgjobs::LEAST_BUSY_REGION,
    &crate::procs::MICROTEX_RENDER,
    &crate::procs::PATH_KIND,
    &crate::jobs_wall_gcloud_trans::wall::RANDOM,
    #[cfg(test)]
    &tests::COUNTER,
    #[cfg(test)]
    &tests::JOB,
    #[cfg(test)]
    &tests::RUNNER,
    &crate::imgjobs::TEXT_COLOR,
    &crate::jobs_wall_gcloud_trans::trans::TRANSLATE,
    &crate::jobs_wall_gcloud_trans::trans::LANGUAGES,
    &crate::jobs_lists::TRANSLATION_LANGUAGES,
    &crate::procs::UPDATES_COUNT,
    &crate::procs::UPDATES_PROBE,
    &crate::procs::WALLPAPER_PICKER,
    &crate::procs::WALLPAPER_SWITCH,
    &crate::procs::WALLPICK_RUN,
    &crate::procs::WEATHER_FETCH,
];

pub fn element(name: &str) -> Option<&'static Element> {
    ELEMENTS.iter().copied().find(|e| e.name == name)
}

/// The element `item` is an instance of.
pub(crate) fn element_of(w: &World, item: usize) -> Option<&'static Element> {
    w.items.get(item)?.types.iter().find_map(|t| element(t))
}

/// A new instance of the type `name`: what its element sets up.
pub(crate) fn created(w: &mut World, item: usize, name: &str) {
    let Some(e) = element(name) else {
        return;
    };
    if e.job.is_some() {
        w.items[item]
            .types
            .push(crate::runtime::Sym::new("Process"));
        w.items[item].tset.set(0);
    }
    if let Some(f) = e.created {
        f(w, item);
    }
}

/// Whether `item` is an element that is a `Process` running a job.
pub(crate) fn has_job(w: &World, item: usize) -> bool {
    element_of(w, item).is_some_and(|e| e.job.is_some())
}

/// The method a spawn hook is told for an element's job: what `running: true` starts.
pub const RUN: &str = "run";

/// What a spawn hook (`native::set_spawn_hook`) gets in place of the job of `item`, an element
/// with one, in the form of [`crate::native::hooked_call`]: the type's name, [`RUN`], then the
/// values of its declared properties in the order of its `.qmltypes` entry (the items of a
/// list in place).
pub(crate) fn hook_argv(w: &mut World, item: usize) -> Vec<String> {
    let Some(e) = element_of(w, item) else {
        return vec![];
    };
    let mut argv = vec![e.name.to_string(), RUN.to_string()];
    if let Some((_, t)) = crate::types::registry().resolve(e.name) {
        for p in &t.props {
            match w.read(item, &p.name).unwrap_or(Value::Undefined) {
                Value::Obj(o) if o.borrow().array.is_some() => {
                    argv.extend(o.borrow().array.iter().flatten().map(|v| v.to_js_string()))
                }
                v => argv.push(v.to_js_string()),
            }
        }
    }
    argv
}

/// Whether `method` of type `name` runs under a [`crate::native::SpawnHook`] (a test,
/// `riaotcheck`). The hook is then told `["Name.method", args…]` and the caller must not
/// touch the real system: no program, thread, bus call, device or file write. What the call
/// would have answered later, it answers at once as a failed run does.
pub(crate) fn hooked(name: &str, method: &str, args: &[&str]) -> bool {
    if !crate::native::sandboxed() {
        return false;
    }
    let mut argv = vec![format!("{name}.{method}")];
    argv.extend(args.iter().map(|a| a.to_string()));
    crate::native::hooked_spawn(&argv);
    true
}

/// The job of `item`, if it is an element with one and it has work to do.
pub(crate) fn job(w: &mut World, item: usize) -> Option<crate::jobs::Job> {
    element_of(w, item)?.job?(w, item)
}

/// `running` of `item` is now `run`; false if `item` is no element that follows it.
pub(crate) fn running(w: &mut World, item: usize, run: bool) -> bool {
    let Some(f) = element_of(w, item).and_then(|e| e.running) else {
        return false;
    };
    f(w, item, run);
    true
}

/// Work a thread sends to the main thread.
pub type Reply = Box<dyn FnOnce(&mut World) + Send>;

/// The way back from a thread to the shell's main loop.
#[derive(Clone)]
pub struct Replies(crate::svc::Sender<Reply>);

impl Replies {
    /// Runs `f` on the main thread at its next turn (the loop is woken for it), where it
    /// writes properties and emits signals. Replies arrive in the order they were sent.
    pub fn send(&self, f: impl FnOnce(&mut World) + Send + 'static) {
        let _ = self.0.send(Box::new(f));
    }
}

/// What a type hands to the thread it starts, to answer with later.
pub fn replies(w: &mut World) -> Replies {
    let waker = &w.waker;
    let (tx, _) = w.impulse_replies.get_or_insert_with(|| {
        let (tx, rx) = waker.handle(crate::svc::Svc::Impulse).channel();
        (Replies(tx), rx)
    });
    tx.clone()
}

/// Runs the replies that arrived; true if there were any.
pub(crate) fn deliver(w: &mut World) -> bool {
    let mut any = false;
    while let Some(f) = w
        .impulse_replies
        .as_ref()
        .and_then(|(_, rx)| rx.try_recv().ok())
    {
        f(w);
        any = true;
    }
    any
}

/// Method `name` of `item`, if `item` is an element of the module that has it; `args: None`
/// only asks whether it has one (`Host::native`).
pub(crate) fn call(
    w: &mut World,
    item: usize,
    name: &str,
    args: Option<&[Value]>,
) -> Option<Value> {
    let (_, f) = *element_of(w, item)?
        .methods
        .iter()
        .find(|(m, _)| *m == name)?;
    Some(match args {
        Some(a) => f(w, item, a),
        None => Value::Undefined,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::runtime::Runtime;

    /// A singleton only the tests declare (`Registry::load` reads it under `cfg(test)`).
    pub(crate) const QMLTYPES: &str = r#"import QtQuick.tooling 1.2
Module {
    Component {
        file: "impulse.rs"
        name: "ImpulseTestProbe"
        accessSemantics: "reference"
        prototype: "QObject"
        exports: ["Impulse/TestProbe 1.0"]
        isCreatable: false
        isSingleton: true
        exportMetaObjectRevisions: [256]
        Property { name: "greeting"; type: "QString"; read: "greeting"; notify: "greetingChanged"; index: 0; isReadonly: true }
        Signal { name: "greetingChanged" }
        Method {
            name: "twice"
            type: "double"
            Parameter { name: "x"; type: "double" }
        }
        Method { name: "undefined" }
        Method { name: "later" }
    }
    Component {
        file: "impulse.rs"
        name: "ImpulseTestJob"
        accessSemantics: "reference"
        prototype: "Process"
        exports: ["Impulse/TestJob 1.0"]
        exportMetaObjectRevisions: [256]
        Property { name: "text"; type: "QString"; read: "text"; write: "setText"; notify: "textChanged"; index: 0 }
        Signal { name: "textChanged" }
    }
    Component {
        file: "impulse.rs"
        name: "ImpulseTestRunner"
        accessSemantics: "reference"
        prototype: "QObject"
        exports: ["Impulse/TestRunner 1.0"]
        exportMetaObjectRevisions: [256]
        Property { name: "running"; type: "bool"; read: "running"; write: "setRunning"; notify: "runningChanged"; index: 0 }
        Property { name: "log"; type: "QString"; read: "log"; notify: "logChanged"; index: 1; isReadonly: true }
        Signal { name: "runningChanged" }
        Signal { name: "logChanged" }
    }
    Component {
        file: "impulse.rs"
        name: "ImpulseTestCounter"
        accessSemantics: "reference"
        prototype: "QObject"
        exports: ["Impulse/TestCounter 1.0"]
        exportMetaObjectRevisions: [256]
        Property { name: "step"; type: "double"; read: "step"; write: "setStep"; notify: "stepChanged"; index: 0 }
        Signal { name: "stepChanged" }
        Method {
            name: "after"
            type: "double"
            Parameter { name: "x"; type: "double" }
        }
    }
}
"#;

    pub(crate) static PROBE: Type = Type {
        name: "TestProbe",
        methods: &[
            ("twice", |_, _, a| {
                Ok(Value::Num(a.first().map_or(0.0, Value::to_number) * 2.0))
            }),
            ("later", |it, _, _| {
                let world = crate::runtime::world_of(it).unwrap();
                let back = replies(&mut world.borrow_mut());
                std::thread::spawn(move || {
                    back.send(|w| {
                        let id = w.singletons["TestProbe"];
                        w.write(id, "greeting", Value::str("later"));
                    });
                });
                Ok(Value::Undefined)
            }),
        ],
        init: Some(|w, id| {
            w.write(id, "greeting", Value::str("hello"));
        }),
    };

    thread_local! {
        static CREATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    pub(crate) static COUNTER: Element = Element {
        methods: &[("after", |w, item, a| {
            let step = w.read(item, "step").map_or(0.0, |v| v.to_number());
            Value::Num(a.first().map_or(0.0, Value::to_number) + step)
        })],
        created: Some(|_, _| CREATED.set(CREATED.get() + 1)),
        ..Element::new("TestCounter")
    };

    pub(crate) static JOB: Element = Element {
        job: Some(|w, item| {
            let text = w.read(item, "text")?.to_js_string();
            (!text.is_empty()).then(|| -> crate::jobs::Job {
                Box::new(move |pipes, _| {
                    pipes.out(format!("{text}\n"));
                    7
                })
            })
        }),
        ..Element::new("TestJob")
    };

    pub(crate) static RUNNER: Element = Element {
        running: Some(|w, item, run| {
            let log = w
                .read(item, "log")
                .map_or(String::new(), |v| v.to_js_string());
            w.write(
                item,
                "log",
                Value::str(&format!("{log}{}", if run { "+" } else { "-" })),
            );
        }),
        ..Element::new("TestRunner")
    };

    /// Runs the native work as it arrives until `done`.
    fn until(rt: &mut Runtime, done: impl Fn(&mut Runtime) -> bool) {
        while !done(rt) {
            rt.wait_native();
            rt.poll_native();
            rt.settle();
        }
    }

    /// Under a [`crate::native::SpawnHook`] (tests, `riaotcheck`) no type of the module
    /// touches the real system: each call is handed to the hook as `Type.method args…` and
    /// what it would have started ends at once, with the signal a failed run gives.
    #[test]
    fn sandboxed_calls_go_to_the_hook_and_end() {
        const DOC: &str = r#"import QtQuick
import Impulse
Item {
    id: root
    property var seen: []
    function note(s) { root.seen = root.seen.concat([s]) }
    HyprlandConfigFile { id: conf; path: "/t/hypr.conf" }
    Connections {
        target: MusicRecognizer
        function onFinished(text, exitCode) { root.note(`finished ${exitCode} ${MusicRecognizer.running}`) }
    }
    Connections {
        target: Keyring
        function onLookupFinished(result, secret) { root.note(`lookup ${result}`) }
        function onUnlockFinished(result) { root.note(`unlock ${result}`) }
    }
    Connections {
        target: DisplayBrightness
        function onDdcDetected(monitors) { root.note(`ddc ${monitors.length}`) }
        function onLevelRead(monitor, ok, current, max) { root.note(`level ${monitor === root} ${ok}`) }
    }
    Connections {
        target: ClipboardHistory
        function onImageDecoded(path, ok) { root.note(`decoded ${path} ${ok}`) }
    }
    function go() {
        SystemSound.play("freedesktop", "bell");
        MusicRecognizer.start(2, 30, "monitor");
        Keyring.lookup({ application: "rusty-impulse" });
        Keyring.store("label", { application: "rusty-impulse" }, "secret");
        Keyring.unlock("password");
        Fingerprint.waitForAvailable();
        DisplayBrightness.detectDdc();
        DisplayBrightness.readBacklight(root);
        DisplayBrightness.readDdc(root, "4");
        DisplayBrightness.setBacklight(0.5);
        DisplayBrightness.setDdc("4", 0.5, 100);
        KeySender.press(30);
        KeySender.release(30);
        KeySender.releaseAll();
        ClipboardHistory.refresh();
        ClipboardHistory.copy("1\ta");
        ClipboardHistory.superpaste(["1\ta"]);
        ClipboardHistory.remove("1\ta");
        ClipboardHistory.wipe();
        ClipboardHistory.decodeImage("1\ta", "/t/a.png");
        ClipboardHistory.removeImage("/t/a.png");
        conf.set({ "general:gaps_in": 4 });
        conf.reset(["general:gaps_in"]);
    }
}
"#;
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let l = log.clone();
        crate::native::set_spawn_hook(Some(Box::new(move |a: &[String]| {
            l.borrow_mut().push(a.join(" "));
            None
        })));
        let mut rt = Runtime::new(None);
        let root = rt.load_str(DOC).unwrap();
        rt.settle();
        let r = rt.call_method(root, "go", vec![]);
        assert!(matches!(r, Some(Ok(_))), "{r:?}");
        // Every answer is already on its way: nothing was started that could give it later.
        rt.poll_native();
        rt.settle();
        crate::native::set_spawn_hook(None);
        assert_eq!(rt.errors(), Vec::<String>::new());
        assert_eq!(
            *log.borrow(),
            [
                "SystemSound.play freedesktop bell",
                "MusicRecognizer.start 2 30 monitor",
                "Keyring.lookup application=rusty-impulse",
                "Keyring.store label application=rusty-impulse",
                "Keyring.unlock",
                "Fingerprint.waitForAvailable",
                "DisplayBrightness.detectDdc",
                "DisplayBrightness.readBacklight",
                "DisplayBrightness.readDdc 4",
                "DisplayBrightness.setBacklight 0.5",
                "DisplayBrightness.setDdc 4 0.5 100",
                "KeySender.press 30",
                "KeySender.release 30",
                "KeySender.releaseAll",
                "ClipboardHistory.refresh",
                "ClipboardHistory.copy 1\ta",
                "ClipboardHistory.superpaste 1\ta",
                "ClipboardHistory.remove 1\ta",
                "ClipboardHistory.wipe",
                "ClipboardHistory.decodeImage 1\ta /t/a.png",
                "ClipboardHistory.removeImage /t/a.png",
                "HyprlandConfigFile.set /t/hypr.conf general:gaps_in=4",
                "HyprlandConfigFile.reset /t/hypr.conf general:gaps_in",
            ]
        );
        let mut seen: Vec<String> = rt
            .get(root, "seen")
            .to_js_string()
            .split(',')
            .map(str::to_owned)
            .collect();
        seen.sort();
        assert_eq!(
            seen,
            [
                "ddc 0",
                "decoded /t/a.png false",
                "finished 1 false",
                "level true false",
                "level true false",
                "lookup 1",
                "unlock 2",
            ]
        );
        // The first check of `Fingerprint.enrolled` ended as well, and nothing waits for a reader.
        let w = rt.world.borrow();
        assert!(w.fprint.as_ref().is_some_and(|f| f.idle() && !f.waiting()));
    }

    #[test]
    fn a_job_element_is_a_process_whose_work_is_its_job() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Rustshell.Io
import Impulse
Item {
    id: root
    property string got: ""
    property int code: -1
    TestJob {
        id: job
        text: "hello"
        stdout: SplitParser { onRead: data => root.got = data }
        onExited: exitCode => root.code = exitCode
    }
    TestJob { id: idle }
}
"#,
            )
            .unwrap();
        rt.settle();
        let (job, idle) = (rt.id("job").unwrap(), rt.id("idle").unwrap());
        rt.set(job, "running", Value::Bool(true));
        until(&mut rt, |rt| rt.get(root, "code").to_number() >= 0.0);
        assert_eq!(rt.get(root, "got").to_js_string(), "hello");
        assert_eq!(rt.get(root, "code").to_number(), 7.0);
        assert!(!rt.get(job, "running").truthy());
        // No job to run: nothing starts.
        rt.set(idle, "running", Value::Bool(true));
        rt.settle();
        assert!(!rt.get(idle, "running").truthy());
    }

    #[test]
    fn a_job_does_not_run_where_commands_do_not() {
        // Without the hook this job prints its text and ends with 7.
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
        let log = asked.clone();
        crate::native::set_spawn_hook(Some(Box::new(move |argv| {
            log.borrow_mut().push(argv.to_vec());
            (argv[2] == "known").then(|| "answer\n".to_string())
        })));
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Rustshell.Io
import Impulse
Item {
    id: root
    property string got: ""
    property var codes: []
    TestJob {
        id: known
        text: "known"
        stdout: SplitParser { onRead: data => root.got += data }
        onExited: exitCode => root.codes = root.codes.concat([exitCode])
    }
    TestJob {
        id: unknown
        text: "unknown"
        stdout: SplitParser { onRead: data => root.got += data }
        onExited: exitCode => root.codes = root.codes.concat([exitCode])
    }
}
"#,
            )
            .unwrap();
        rt.settle();
        for (id, code) in [("known", "0"), ("unknown", "0,1")] {
            let job = rt.id(id).unwrap();
            rt.set(job, "running", Value::Bool(true));
            until(&mut rt, |rt| rt.get(root, "codes").to_js_string() == code);
            assert!(!rt.get(job, "running").truthy());
        }
        crate::native::set_spawn_hook(None);
        // The hook got each element as its name, `run` and its property values, and answered
        // for it.
        assert_eq!(
            *asked.borrow(),
            [["TestJob", "run", "known"], ["TestJob", "run", "unknown"]]
        );
        assert_eq!(rt.get(root, "got").to_js_string(), "answer");
    }

    /// Each native type is a type of the module's table: a script's write asks the table
    /// which members the type has.
    #[test]
    fn every_native_type_is_in_the_table() {
        let r = crate::types::registry();
        let names = (TYPES.iter().map(|t| t.name)).chain(ELEMENTS.iter().map(|e| e.name));
        for name in names {
            assert_eq!(r.resolve(name).map(|(m, _)| m), Some("Impulse"), "{name}");
        }
        for e in ELEMENTS {
            if e.running.is_some() || e.job.is_some() {
                let ty = [crate::runtime::Sym::new(e.name)];
                let running = crate::types::find_prop(&ty, "running");
                assert!(running.is_some(), "{}: running", e.name);
            }
        }
    }

    /// Both lists are in the order of the types' QML names, each name once.
    #[test]
    fn the_lists_are_sorted_by_name() {
        let types: Vec<&str> = TYPES.iter().map(|t| t.name).collect();
        let elements: Vec<&str> = ELEMENTS.iter().map(|e| e.name).collect();
        for names in [types, elements] {
            let mut sorted = names.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(names, sorted);
        }
    }

    /// An element's property is its own: one it leaves unset is not taken from an object
    /// around it that has a property of the same name.
    #[test]
    fn an_unset_property_is_not_read_from_an_ancestor() {
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
        let log = asked.clone();
        crate::native::set_spawn_hook(Some(Box::new(move |argv| {
            log.borrow_mut().push(argv.to_vec());
            Some(String::new())
        })));
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Impulse
Item {
    id: root
    property string text: "outer"
    property int code: -1
    Item {
        property string text: "nearer"
        TestJob {
            id: job
            onExited: exitCode => root.code = exitCode
        }
    }
}
"#,
            )
            .unwrap();
        rt.settle();
        let job = rt.id("job").unwrap();
        rt.set(job, "running", Value::Bool(true));
        until(&mut rt, |rt| rt.get(root, "code").to_number() >= 0.0);
        crate::native::set_spawn_hook(None);
        assert_eq!(*asked.borrow(), [["TestJob", "run", ""]]);
    }

    #[test]
    fn an_element_follows_its_running_property() {
        let mut rt = Runtime::new(None);
        rt.load_str(
            r#"import QtQuick
import Impulse
Item {
    TestRunner { id: started; running: true }
    TestRunner { id: idle }
}
"#,
        )
        .unwrap();
        rt.settle();
        let (started, idle) = (rt.id("started").unwrap(), rt.id("idle").unwrap());
        assert_eq!(rt.get(started, "log").to_js_string(), "+");
        assert_eq!(rt.get(idle, "log").to_js_string(), "");
        rt.set(started, "running", Value::Bool(false));
        rt.set(idle, "running", Value::Bool(true));
        rt.settle();
        assert_eq!(rt.get(started, "log").to_js_string(), "+-");
        assert_eq!(rt.get(idle, "log").to_js_string(), "+");
    }

    #[test]
    fn a_thread_answers_a_singleton_on_the_main_thread() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Impulse
Item {
    property string greeting: TestProbe.greeting
    Component.onCompleted: TestProbe.later()
}
"#,
            )
            .unwrap();
        rt.settle();
        until(&mut rt, |rt| {
            rt.get(root, "greeting").to_js_string() == "later"
        });
    }

    #[test]
    fn a_created_element_gets_its_methods_from_its_element() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Impulse
Item {
    property real a: first.after(1)
    property real b: second.after(1)
    property bool other: typeof first.twice === "function"
    TestCounter { id: first; step: 2 }
    TestCounter { id: second; step: 40 }
}
"#,
            )
            .unwrap();
        rt.settle();
        assert_eq!(rt.get(root, "a").to_number(), 3.0);
        assert_eq!(rt.get(root, "b").to_number(), 41.0);
        assert!(!rt.get(root, "other").truthy());
        assert_eq!(CREATED.get(), 2);
    }

    /// QML names the singleton, reads a property `init` set and calls a method of the table;
    /// a declared method with no function says so by name.
    #[test]
    fn a_singleton_gets_its_methods_and_initial_properties_from_its_type() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Impulse
Item {
    property real doubled: TestProbe.twice(21)
    property string greeting: TestProbe.greeting
    property var missing: TestProbe.undefined()
}
"#,
            )
            .unwrap();
        rt.settle();
        assert_eq!(rt.get(root, "doubled").to_number(), 42.0);
        assert_eq!(rt.get(root, "greeting").to_js_string(), "hello");
        assert!(matches!(rt.get(root, "missing"), Value::Undefined));
        let errors = rt.errors();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].ends_with(&crate::unwritten::message("TestProbe", "undefined")),
            "{errors:?}"
        );
    }
}
