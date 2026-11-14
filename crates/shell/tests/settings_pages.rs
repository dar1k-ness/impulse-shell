//! What the tree's settings pages show and do for a given config.json: the wallpaper tiles load
//! a video wallpaper's still frame, every position of the volume limit slider is a volume the
//! shell lets through, a row whose sub-options are all hidden (a SettingsPair hides its rows by
//! its own `shown`) has no fold arrow, and "Copy info" on the About page copies without opening
//! a URL.

use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const SIZE: [f64; 2] = [1120.0, 780.0];

/// A 2x2 PNG.
const PNG: [u8; 74] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0xfd, 0xd4, 0x9a,
    0x73, 0x00, 0x00, 0x00, 0x11, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x44, 0x0c, 0x60, 0xf2, 0x3f, 0x00, 0x1b, 0xf2, 0x03, 0xfd, 0xf3, 0xe7, 0x4b, 0xb1, 0x00, 0x00,
    0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap()
}

struct Win {
    rt: Runtime,
    root: usize,
    fonts: scene::Fonts,
    now: f64,
    wake: testenv::Wakeups,
}

impl Win {
    /// One frame: native events, the animation clock 16 ms on, bindings, layout.
    fn frame(&mut self) {
        self.rt.poll_native();
        self.now += 16.0;
        self.rt.tick(self.now);
        self.rt.settle();
        scene::layout_window(&mut self.rt, &mut self.fonts, self.root, SIZE);
        self.rt.settle();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    fn text(&self, i: usize, k: &str) -> String {
        self.rt.get(i, k).to_js_string()
    }

    /// The live items for which `f` holds.
    fn all(&self, f: impl Fn(&mut World, usize) -> bool) -> Vec<usize> {
        let mut w = self.rt.world.borrow_mut();
        (0..w.items.len())
            .filter(|&i| !w.dead.contains(&i) && f(&mut w, i))
            .collect()
    }

    /// The settings row titled `title`.
    fn row(&self, title: &str) -> usize {
        let hits = self.all(|w, i| is_row(w, i) && titled(w, i, title));
        assert_eq!(hits.len(), 1, "rows titled {title}: {hits:?}");
        hits[0]
    }

    /// The sub-option titled `title` of the row `of`.
    fn sub_row(&self, of: usize, title: &str) -> usize {
        let hits = self.all(|w, i| {
            is_row(w, i)
                && titled(w, i, title)
                && matches!(w.read(i, "parentRow"), Some(Value::Item(p)) if p == of)
        });
        assert_eq!(hits.len(), 1, "sub-options titled {title}: {hits:?}");
        hits[0]
    }

    /// The live items inside the settings row `row` (not inside a row of its own) for which
    /// `f` holds.
    fn in_row(&self, row: usize, f: impl Fn(&mut World, usize) -> bool) -> Vec<usize> {
        self.all(|w, i| {
            std::iter::successors(w.items[i].parent, |&p| w.items[p].parent).find(|&p| is_row(w, p))
                == Some(row)
                && f(w, i)
        })
    }
}

fn is_row(w: &World, i: usize) -> bool {
    w.items[i].props.contains_key("childRows")
}

fn titled(w: &mut World, i: usize, title: &str) -> bool {
    w.read(i, "title")
        .is_some_and(|t| t.to_js_string() == title)
}

fn shows(w: &mut World, i: usize, text: &str) -> bool {
    w.read(i, "text").is_some_and(|t| t.to_js_string() == text)
}

fn is(w: &World, i: usize, t: &str) -> bool {
    w.items[i].types.iter().any(|x| x == t)
}

/// The FIFO under HOME that the `xdg-open` of [`settings`] writes its argument to.
const OPENED: &str = "xdg-open.calls";

/// The settings window with every page built, in `test`'s own process (see
/// `testenv::run_in_own_process`) with a fresh HOME whose config.json is `config(home)`;
/// `None` in the process that started it.
fn settings(test: &str, config: impl Fn(&Path) -> String) -> Option<Win> {
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        std::fs::create_dir_all(h.join(".config/rusty-impulse")).unwrap();
        std::fs::create_dir_all(h.join(".local/state/rustshell/user")).unwrap();
        std::fs::create_dir_all(h.join("bin")).unwrap();
        std::fs::write(h.join(".config/rusty-impulse/config.json"), config(h)).unwrap();
        // Whatever a page opens ends here, not in a browser: in the FIFO a test made to read
        // what was opened ([`OPENED`]), nowhere without one.
        let opened = format!(
            "#!/bin/sh\nif [ -p \"$HOME/{OPENED}\" ]; then echo \"$1\" > \"$HOME/{OPENED}\"; fi\n"
        );
        testenv::install_script(&h.join("bin/xdg-open"), &opened);
        let mut path = h.join("bin").into_os_string();
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        let mut env = testenv::home_vars(h);
        env.push(("PATH", path));
        testenv::run_in_own_process(test, env);
        return None;
    }
    let dir = ri();
    let mut rt = Runtime::new(Some(dir.clone()));
    // As the shell: items are laid out after creation, Behaviors arm after the first layout.
    rt.world.borrow_mut().track_fresh = true;
    // Image loads report on channels that wake the world's services.
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let dirs: Vec<PathBuf> = [
        "modules/common",
        "modules/common/functions",
        "modules/common/utils",
        "services",
        "",
    ]
    .iter()
    .map(|d| dir.join(d))
    .collect();
    rt.load_singletons_lazy(&dirs);
    let root = rt.load_file(&dir.join("settings.qml")).unwrap();
    rt.settle();
    let mut win = Win {
        rt,
        root,
        fonts: scene::Fonts::default(),
        now: 0.0,
        wake,
    };
    win.frames(90);
    Some(win)
}

/// A video wallpaper is no picture an Image can load: the wallpaper tiles (the Quick page's
/// wallpaper, the bar position tiles) show its still frame, `background.thumbnailPath`. They
/// were given the video itself and stayed empty.
#[test]
fn wallpaper_tiles_show_a_video_wallpapers_still_frame() {
    let test = concat!(
        module_path!(),
        "::wallpaper_tiles_show_a_video_wallpapers_still_frame"
    );
    let Some(mut win) = settings(test, |home| {
        std::fs::write(home.join("wall.mp4"), b"not a picture").unwrap();
        std::fs::write(home.join("still.png"), PNG).unwrap();
        format!(
            r#"{{"background": {{"wallpaperPath": "{0}/wall.mp4", "thumbnailPath": "{0}/still.png"}}}}"#,
            home.display()
        )
    }) else {
        return;
    };
    let images = win.all(|w, i| {
        is(w, i, "Image")
            && w.items[i]
                .parent
                .is_some_and(|p| is(w, p, "SettingsWallpaperThumb"))
    });
    assert!(
        images.len() > 1,
        "fixture: the wallpaper tile and the bar position tiles: {images:?}"
    );
    // Image.Loading ends on a report from the pool: nothing to wait for but that.
    // Image.Loading ends when the decode lands, which pings. The readiness is taken before
    // the loads are drained: a report after the take pings.
    loop {
        win.rt.world.borrow().waker.take();
        let (hit, _) = scene::take_finished_images(&mut win.rt.world.borrow_mut());
        if !hit.is_empty() {
            scene::relayout_targets(&mut win.rt.world.borrow_mut(), &mut win.fonts, &hit, 0);
            win.rt.settle();
        }
        if images
            .iter()
            .all(|&i| win.rt.get(i, "status").to_number() != 2.0)
        {
            break;
        }
        win.wake.wait(None, None);
    }
    for &i in &images {
        let source = win.text(i, "source");
        assert!(source.ends_with("/still.png"), "tile {i} shows {source}");
        assert_eq!(
            win.rt.get(i, "status").to_number(),
            1.0,
            "Image.Ready of {source}"
        );
    }
}

/// "Volume limit" goes to 154% (pavucontrol's 153%), below the 200% no volume may pass
/// (`Audio.hardMaxValue`), so every position of the slider is a limit that holds, and a
/// stored 150 shows as 150%. The hard maximum was 100%: the slider's upper third did nothing.
#[test]
fn every_volume_limit_is_below_the_hard_maximum() {
    let test = concat!(
        module_path!(),
        "::every_volume_limit_is_below_the_hard_maximum"
    );
    let Some(mut win) = settings(test, |_| {
        r#"{"audio": {"protection": {"enable": true, "maxAllowed": 150}}}"#.into()
    }) else {
        return;
    };
    let hard = win
        .rt
        .load_str("import QtQuick\nimport ri.services\nItem { property real hard: Audio.hardMaxValue * 100 }\n")
        .unwrap_or_else(|e| panic!("{e}"));
    let hard = win.rt.get(hard, "hard").to_number();
    let row = win.row("Volume limit");
    let to = win.rt.get(row, "to").to_number();
    assert_eq!(to, 154.0);
    assert!(to <= hard, "the slider ends at {to}%, volume at {hard}%");
    let percent = win.in_row(row, |w, i| {
        is(w, i, "StyledText")
            && w.read(i, "text")
                .is_some_and(|t| t.to_js_string().ends_with('%'))
    });
    let shown: Vec<String> = percent.iter().map(|&i| win.text(i, "text")).collect();
    assert!(
        !shown.is_empty() && shown.iter().all(|t| t == "150%"),
        "the value the row shows: {shown:?}"
    );
}

/// The fold arrow of a row is there while one of its sub-options shows. Rows in a SettingsPair
/// are hidden by the pair's `shown`, their own stays true: "Show clock" and "Enable overview"
/// kept the arrow with nothing under them.
#[test]
fn a_row_with_every_sub_option_hidden_has_no_fold_arrow() {
    let test = concat!(
        module_path!(),
        "::a_row_with_every_sub_option_hidden_has_no_fold_arrow"
    );
    let Some(win) = settings(test, |_| {
        r#"{"background": {"widgets": {"clock": {"enable": false}}},
            "overview": {"enable": false},
            "audio": {"protection": {"enable": true}}}"#
            .into()
    }) else {
        return;
    };
    let arrow = |title: &str| {
        let arrows = win.in_row(win.row(title), |w, i| {
            is(w, i, "MaterialSymbol") && shows(w, i, "expand_more")
        });
        assert_eq!(arrows.len(), 1, "fold arrows of {title}: {arrows:?}");
        let toggle = win.rt.world.borrow().items[arrows[0]].parent.unwrap();
        win.rt.get(toggle, "visible").truthy()
    };
    for (of, title) in [
        ("Show clock", "Vertical"),
        ("Show clock", "Show date"),
        ("Enable overview", "Rows"),
        ("Enable overview", "Columns"),
    ] {
        let row = win.sub_row(win.row(of), title);
        let pair = win.rt.world.borrow().items[row].parent.unwrap();
        assert!(
            win.rt.get(pair, "settingsPair").truthy(),
            "fixture: {title}"
        );
        assert!(win.rt.get(row, "shown").truthy(), "fixture: {title}");
        assert!(!win.rt.get(pair, "shown").truthy(), "{title} is hidden");
    }
    assert!(!arrow("Show clock"));
    assert!(!arrow("Enable overview"));
    // Sub-options that show keep it.
    assert!(arrow("Earbang protection"));
}

/// "Copy info" on the About page copies the line and says so. It was a link button without a
/// URL, whose own click handler also opened the empty URL.
#[test]
fn copy_info_opens_no_url() {
    let test = concat!(module_path!(), "::copy_info_opens_no_url");
    let Some(mut win) = settings(test, |_| "{}\n".into()) else {
        return;
    };
    let label = |w: &mut World, i: usize, text: &str| {
        w.read(i, "mainText")
            .is_some_and(|t| t.to_js_string() == text)
    };
    let buttons = win.all(|w, i| label(w, i, "Copy info"));
    assert_eq!(buttons.len(), 1, "{buttons:?}");
    // Held open for writing too: `xdg-open` never waits for a reader, and a read waits for
    // the next URL instead of ending at the last writer's close.
    let calls = PathBuf::from(std::env::var_os("HOME").unwrap()).join(OPENED);
    testenv::fifo(&calls);
    let calls = (std::fs::OpenOptions::new().read(true).write(true))
        .open(&calls)
        .unwrap();
    let mut calls = std::io::BufReader::new(calls);
    win.rt.emit(buttons[0], "clicked", &[], vec![]);
    win.rt.settle();
    assert_eq!(win.text(buttons[0], "mainText"), "Copied");
    // A link button still opens its URL, and it is the first one opened.
    let docs = win.all(|w, i| label(w, i, "Docs"));
    assert_eq!(docs.len(), 1, "{docs:?}");
    win.rt.emit(docs[0], "clicked", &[], vec![]);
    win.rt.settle();
    let url = win.text(docs[0], "url");
    assert!(url.starts_with("https://"), "fixture: {url}");
    let mut opened = String::new();
    std::io::BufRead::read_line(&mut calls, &mut opened).unwrap();
    assert_eq!(opened.trim_end(), url);
}
