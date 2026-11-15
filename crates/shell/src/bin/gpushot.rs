//! Offscreen renders of the original windows through headless EGL, for comparing GL drivers on machines
//! without a display (tools/reference/kaggle-gpu: Kaggle's NVIDIA GPUs against llvmpipe). Each frame is
//! laid out and painted once and the same draw list goes to every backend's pbuffer window
//! through `Gpu::render`, theme crossfades included, on a virtual clock: clocks, sensors and
//! timing cannot make the backends' frames differ, only the drivers do.
//!
//! usage: gpushot RI_ROOT OUT_DIR [--scene bar|settings] [--egl SPEC]... [--raster]
//!        [--theme FROM.json TO.json] [--frames N] [--step MS] [--tol T] [--bench N]
//! SPEC: auto, device, device:N, software, surfaceless, default (`Gpu::headless`).
//! Writes OUT_DIR/<backend>/<frame>.png (premultiplied RGBA), OUT_DIR/diff/<frame>.png for
//! frames that differ from the reference (the last backend) and OUT_DIR/report.json. Exit
//! status 3 when any frame has more than `--max-px` pixels off by more than `--tol`.
use qsg::raster::Canvas;
use shell::{gpu, scene};
use std::path::{Path, PathBuf};

const STEP: f64 = 16.0;

struct Args {
    root: PathBuf,
    out: PathBuf,
    scene: String,
    egl: Vec<String>,
    raster: bool,
    theme: Option<(PathBuf, PathBuf)>,
    frames: usize,
    step: f64,
    tol: u8,
    max_px: usize,
    /// Re-render the steady frame this many times per backend, with and without the GL state
    /// cache, printing the time and GL calls per frame.
    bench: usize,
}

fn args() -> Args {
    let mut a = std::env::args().skip(1);
    let usage = "usage: gpushot RI_ROOT OUT_DIR [--scene bar|settings] [--egl SPEC]... [--raster] \
                 [--theme FROM.json TO.json] [--frames N] [--step MS] [--tol T] [--max-px N] \
                 [--bench N]";
    let (Some(root), Some(out)) = (a.next(), a.next()) else {
        elog::eprintln!("{usage}");
        std::process::exit(2);
    };
    let mut r = Args {
        root: PathBuf::from(root),
        out: PathBuf::from(out),
        scene: "bar".into(),
        egl: Vec::new(),
        raster: false,
        theme: None,
        frames: 30,
        step: 16.0,
        tol: 2,
        max_px: 0,
        bench: 0,
    };
    let val = |n: &str, a: &mut dyn Iterator<Item = String>| {
        a.next().unwrap_or_else(|| {
            elog::eprintln!("{n} needs a value\n{usage}");
            std::process::exit(2)
        })
    };
    while let Some(k) = a.next() {
        match k.as_str() {
            "--scene" => r.scene = val(&k, &mut a),
            "--egl" => r.egl.push(val(&k, &mut a)),
            "--raster" => r.raster = true,
            "--theme" => {
                let from = val(&k, &mut a);
                r.theme = Some((from.into(), val(&k, &mut a).into()));
            }
            "--frames" => r.frames = val(&k, &mut a).parse().expect("--frames"),
            "--step" => r.step = val(&k, &mut a).parse().expect("--step"),
            "--bench" => r.bench = val(&k, &mut a).parse().expect("--bench"),
            "--tol" => r.tol = val(&k, &mut a).parse().expect("--tol"),
            "--max-px" => r.max_px = val(&k, &mut a).parse().expect("--max-px"),
            _ => {
                elog::eprintln!("unknown argument {k}\n{usage}");
                std::process::exit(2);
            }
        }
    }
    if r.egl.is_empty() {
        r.egl.push("auto".into());
    }
    r
}

/// A private HOME/XDG tree, so the run reads no user config and its palette file is ours.
fn isolate(out: &Path) -> PathBuf {
    let home = out.join("home");
    for d in [".config", ".local/state", ".cache", ".local/share", "run"] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    for (k, v) in [
        ("HOME", home.clone()),
        ("XDG_RUNTIME_DIR", home.join("run")),
        ("XDG_CONFIG_HOME", home.join(".config")),
        ("XDG_STATE_HOME", home.join(".local/state")),
        ("XDG_CACHE_HOME", home.join(".cache")),
        ("XDG_DATA_HOME", home.join(".local/share")),
    ] {
        #[expect(
            clippy::disallowed_methods,
            reason = "before the threads that read the environment start"
        )]
        // SAFETY: no other thread exists yet.
        unsafe {
            std::env::set_var(k, v)
        };
    }
    // Config.ready (and with it the original family) waits for this file.
    let cfg = home.join(".config/impulse-shell/config.json");
    std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    if !cfg.exists() {
        std::fs::write(&cfg, "{}\n").unwrap();
    }
    home.join(".local/state/impulse-shell/user/generated/colors.json")
}

fn set_palette(colors: &Path, from: &Path) {
    std::fs::create_dir_all(colors.parent().unwrap()).unwrap();
    let tmp = colors.with_extension("tmp");
    std::fs::copy(from, &tmp).unwrap_or_else(|e| panic!("{}: {e}", from.display()));
    std::fs::rename(&tmp, colors).unwrap();
}

fn runtime(
    root: &Path,
    fonts: &std::rc::Rc<std::cell::RefCell<scene::Fonts>>,
) -> qml::runtime::Runtime {
    let mut rt = qml::runtime::Runtime::new(Some(root.to_path_buf()));
    rt.world.borrow_mut().track_fresh = !qml::debug::on(qml::debug::Switch::NOFRESH);
    rt.world.borrow_mut().frame_paced = !qml::debug::on(qml::debug::Switch::NOPOLISH);
    if !cfg!(feature = "interp") || !shell::env_flag!("RI_INTERP") {
        rt.eng()
            .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(root))));
    }
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().pre_complete = Some(Box::new(move |w, n0| {
            fonts
                .try_borrow_mut()
                .is_ok_and(|mut f| scene::measure_new(w, &mut f, n0))
        }));
    }
    scene::loaded_layout_hook(&mut rt.world.borrow_mut(), fonts);
    scene::image_hook(&mut rt.world.borrow_mut(), fonts);
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().text_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = fonts.try_borrow_mut() {
                scene::measure_text(w, &mut f, i);
            }
        }));
    }
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().popup_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = fonts.try_borrow_mut() {
                scene::measure_popup(w, &mut f, i);
            }
        }));
    }
    let mut dirs = vec![
        root.join("modules/common"),
        root.join("modules/common/functions"),
        root.join("modules/common/utils"),
        root.join("services"),
        root.to_path_buf(),
    ];
    for fam in ["modules/rusty", "modules/waffle"] {
        let mut d: Vec<_> = std::fs::read_dir(root.join(fam))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        d.sort();
        dirs.extend(d);
    }
    rt.load_singletons_lazy(&dirs);
    rt.set_screens_dpr(&[("DP-1".into(), 0, 0, 1920, 1080, 1.0)]);
    rt
}

/// Runs `ms` of virtual time in frames, taking the native work (file events, process output)
/// that is ready at each, as the shell's loop does when the runtime's fds wake it.
fn run(rt: &mut qml::runtime::Runtime, now: &mut f64, ms: f64) {
    let end = *now + ms;
    while *now < end {
        settle(rt, *now);
        *now += STEP;
        rt.tick(*now);
    }
}

/// Turns at `now` until no native work is ready.
fn settle(rt: &mut qml::runtime::Runtime, now: f64) {
    while rt.native_ready() {
        rt.poll_native();
        rt.tick(now);
    }
}

/// Drops the recorded draws that the runtime's changes since the last frame made stale, as the
/// shell does before each frame.
fn invalidate(rt: &mut qml::runtime::Runtime) {
    let (restructured, moved, touched) = {
        let mut w = rt.world.borrow_mut();
        let w = &mut *w;
        (
            std::mem::take(&mut w.restructured),
            std::mem::take(&mut w.moved),
            std::mem::take(&mut w.touched),
        )
    };
    let w = rt.world.borrow();
    if !restructured.is_empty() {
        scene::invalidate_parents(&w, &restructured);
    }
    if !moved.is_empty() {
        scene::invalidate_moved(&w, &moved);
    }
    if !touched.is_empty() {
        scene::invalidate_layers(&w, &touched);
    }
}

fn window(rt: &mut qml::runtime::Runtime, scene_name: &str) -> (usize, [f64; 2]) {
    if scene_name == "bar" {
        let win = *scene::find_windows(rt, "rustshell:bar")
            .first()
            .expect("no bar window");
        return (win, [1920.0, rt.get(win, "implicitHeight").to_number()]);
    }
    let wins = rt.world.borrow_mut().of_type("ApplicationWindow");
    let win = *wins.first().expect("no ApplicationWindow");
    let dim = |rt: &qml::runtime::Runtime, k: &str, ik: &str| {
        let v = rt.get(win, k).to_number();
        if v > 0.0 {
            v
        } else {
            rt.get(win, ik).to_number()
        }
    };
    let size = [
        dim(rt, "width", "implicitWidth"),
        dim(rt, "height", "implicitHeight"),
    ];
    (win, size)
}

struct FrameDiff {
    frame: String,
    backend: String,
    pixels: usize,
    over_tol: usize,
    max_delta: u8,
    mean_delta: f64,
}

fn compare(
    name: &str,
    backend: &str,
    a: &Canvas,
    b: &Canvas,
    tol: u8,
    diff_dir: &Path,
) -> FrameDiff {
    assert_eq!((a.w, a.h), (b.w, b.h));
    let (mut over, mut max, mut sum) = (0, 0u8, 0u64);
    let mut img = Canvas::new(a.w, a.h, [0, 0, 0, 255]);
    for (k, (p, q)) in a.px.iter().zip(&b.px).enumerate() {
        let d = (0..4).map(|c| p[c].abs_diff(q[c])).max().unwrap();
        sum += d as u64;
        max = max.max(d);
        if d > tol {
            over += 1;
            img.px[k] = [255, 0, (d as u16 * 4).min(255) as u8, 255];
        } else {
            let g = q[3] / 4;
            img.px[k] = [g, g, g, 255];
        }
    }
    if over > 0 {
        img.save_png(
            diff_dir
                .join(format!("{name}-{backend}.png"))
                .to_str()
                .unwrap(),
        )
        .unwrap();
    }
    FrameDiff {
        frame: name.into(),
        backend: backend.into(),
        pixels: a.px.len(),
        over_tol: over,
        max_delta: max,
        mean_delta: sum as f64 / a.px.len().max(1) as f64,
    }
}

fn main() {
    qml::debug::init_log();
    let a = args();
    std::fs::create_dir_all(&a.out).unwrap();
    let out = a.out.canonicalize().unwrap();
    let root = a.root.canonicalize().expect("RI_ROOT");
    let colors = isolate(&out);
    if let Some((from, _)) = &a.theme {
        set_palette(&colors, from);
    }
    type Win = Option<(gpu::Gpu, Option<gpu::Surface>)>;
    let mut backends: Vec<(String, Win)> = Vec::new();
    let mut info = Vec::new();
    for spec in &a.egl {
        let (g, what) = gpu::Gpu::headless(spec).unwrap_or_else(|e| {
            elog::eprintln!("gpushot: EGL {spec}: {e}");
            std::process::exit(4);
        });
        println!("backend {spec}: {what}, GL_RENDERER {}", g.renderer);
        let name = spec.replace(':', "");
        info.push(serde_json::json!({"name": name, "egl": what, "renderer": g.renderer}));
        backends.push((name, Some((g, None))));
    }
    if a.raster {
        info.push(serde_json::json!({"name": "raster", "egl": null, "renderer": "qsg raster"}));
        backends.push(("raster".into(), None));
    }
    let names: Vec<String> = backends.iter().map(|b| b.0.clone()).collect();
    if names.iter().collect::<std::collections::HashSet<_>>().len() != names.len() {
        elog::eprintln!("gpushot: backends must differ: {names:?}");
        std::process::exit(2);
    }
    for n in names.iter().chain(["diff".to_string()].iter()) {
        std::fs::create_dir_all(out.join(n)).unwrap();
    }

    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    let mut rt = runtime(&root, &fonts);
    let file = match a.scene.as_str() {
        // The whole shell, as started: its startup applies the theme the bar draws with.
        "bar" => root.join("shell.qml"),
        "settings" => root.join("settings.qml"),
        s => {
            elog::eprintln!("gpushot: unknown scene {s}");
            std::process::exit(2);
        }
    };
    rt.load_file(&file)
        .unwrap_or_else(|e| panic!("load {}: {e}", file.display()));
    let mut now = 1000.0;
    run(&mut rt, &mut now, 3000.0);

    let mut diffs = Vec::new();
    let t0 = std::time::Instant::now();
    let mut prev: Option<gpu::Frame> = None;
    let mut frame = |rt: &mut qml::runtime::Runtime, now: f64, name: &str| {
        invalidate(rt);
        let (win, size) = window(rt, &a.scene);
        let mut f = fonts.borrow_mut();
        scene::layout_window(rt, &mut f, win, size);
        let (w, h) = (size[0].ceil() as usize, size[1].ceil() as usize);
        let mut fr = gpu::Frame::scaled(w, h, 1.0);
        scene::paint_window(rt, &mut f, win, &mut scene::Target::Gpu(&mut fr));
        // Counted as the shell counts its frames: output held for this window's next frame is
        // due at the next poll.
        rt.set_frame_wins([win]);
        {
            let mut wd = rt.world.borrow_mut();
            wd.frame_seq += 1;
            let seq = wd.frame_seq;
            wd.win_frames.insert(win, seq);
        }
        let need_cpu = backends.iter().any(|b| b.1.is_none());
        let cpu = need_cpu.then(|| {
            let mut cv = Canvas::new(w, h, [0, 0, 0, 0]);
            scene::paint_window(rt, &mut f, win, &mut scene::Target::Cpu(&mut cv));
            cv
        });
        let clock = t0 + std::time::Duration::from_secs_f64(now / 1000.0);
        let shots: Vec<Canvas> = backends
            .iter_mut()
            .map(|(n, b)| {
                let Some((g, surf)) = b else {
                    return cpu.clone().unwrap();
                };
                if surf
                    .as_ref()
                    .is_none_or(|s| s.size() != (w as i32, h as i32))
                {
                    *surf = Some(g.pbuffer(w as i32, h as i32).unwrap_or_else(|| {
                        elog::eprintln!("gpushot: {n}: no {w}x{h} pbuffer");
                        std::process::exit(4)
                    }));
                }
                let surf = surf.as_mut().unwrap();
                g.clock = Some(clock);
                // Every frame is read back, so none may be skipped as unchanged.
                g.may_skip = false;
                // As the shell does: a theme change crossfades from the window's last frame.
                surf.crossfade.observe(true);
                if let Some(d) = surf
                    .crossfade
                    .take(|| prev.as_ref().map(|p| !p.draws_same(&fr)))
                    && let Some(old) = &prev
                {
                    g.begin_fade(surf, old, d, fr.changed_part(old).as_deref());
                }
                g.render(surf, &mut fr.clone(), || {});
                g.read_surface(surf)
            })
            .collect();
        prev = Some(fr);
        for (n, c) in names.iter().zip(&shots) {
            c.save_png(out.join(n).join(format!("{name}.png")).to_str().unwrap())
                .unwrap();
        }
        let reference = shots.last().unwrap();
        for (n, c) in names.iter().zip(&shots).take(shots.len() - 1) {
            let d = compare(name, n, c, reference, a.tol, &out.join("diff"));
            println!(
                "frame {name} {n} vs {}: {} px over tol {} (max delta {}, mean {:.4})",
                names.last().unwrap(),
                d.over_tol,
                a.tol,
                d.max_delta,
                d.mean_delta
            );
            diffs.push(d);
        }
        shots
    };

    let start = frame(&mut rt, now, "steady");
    let mut theme_frames = 0;
    if let Some((_, to)) = &a.theme {
        let gen0 = qml::native::crossfade_gen();
        let seen = rt.file_events();
        set_palette(&colors, to);
        // The watch's event is queued once rename(2) returns: block on the runtime's fds until
        // a poll takes it, then run the reload and the crossfade request it brings, at this
        // frame; frames start with the first tick that has them.
        while rt.file_events() == seen {
            rt.wait_native();
            rt.poll_native();
            rt.tick(now);
        }
        settle(&mut rt, now);
        let faded = qml::native::crossfade_gen() != gen0;
        println!("theme switch requested a crossfade: {faded}");
        now += a.step;
        rt.tick(now);
        for k in 0..a.frames {
            frame(&mut rt, now, &format!("theme{k:03}"));
            rt.poll_native();
            now += a.step;
            rt.tick(now);
        }
        theme_frames = a.frames;
        run(&mut rt, &mut now, 3000.0);
        let end = frame(&mut rt, now, "themed");
        // A palette switch that never reached the window would compare equal everywhere.
        let changed = start
            .last()
            .unwrap()
            .px
            .iter()
            .zip(&end.last().unwrap().px)
            .filter(|(p, q)| p != q)
            .count();
        println!("theme switch changed {changed} px of the steady frame");
        if changed == 0 {
            elog::eprintln!("gpushot: the palette switch did not change the window");
            std::process::exit(5);
        }
    }
    if a.bench > 0 {
        let fr = prev.clone().expect("steady frame");
        for (n, b) in backends.iter_mut() {
            let Some((g, Some(surf))) = b else { continue };
            for cache in [true, false] {
                g.gl_cache = cache;
                g.may_skip = false;
                surf.reswap();
                g.render(surf, &mut fr.clone(), || {});
                g.finish();
                let _ = gpu::take_gl_calls();
                let mut submit = 0.0;
                let t = std::time::Instant::now();
                for _ in 0..a.bench {
                    surf.reswap();
                    let t1 = std::time::Instant::now();
                    g.render(surf, &mut fr.clone(), || {});
                    submit += t1.elapsed().as_secs_f64();
                    g.finish();
                }
                let wall = t.elapsed().as_secs_f64();
                let c = gpu::take_gl_calls().map(|c| c / a.bench as u64);
                let per = 1000.0 / a.bench as f64;
                println!(
                    "bench {n} gl_cache={cache}: {} frames, {:.3} ms/frame render+finish, \
                     {:.3} ms/frame cpu submit, gl calls/frame: program={} uniform={} \
                     texparam={} attrib={} bindbuf={} teximage={} subimage={}",
                    a.bench,
                    wall * per,
                    submit * per,
                    c[3],
                    c[4],
                    c[5],
                    c[6],
                    c[7],
                    c[0],
                    c[1]
                );
            }
            g.gl_cache = true;
        }
    }
    let errors: Vec<String> = rt.errors().iter().map(|e| e.to_string()).collect();
    let bad = diffs.iter().filter(|d| d.over_tol > a.max_px).count();
    let report = serde_json::json!({
        "scene": a.scene,
        "backends": info,
        "reference": names.last(),
        "tol": a.tol,
        "max_px": a.max_px,
        "theme_frames": theme_frames,
        "step_ms": a.step,
        "frames": diffs.iter().map(|d| serde_json::json!({
            "frame": d.frame,
            "backend": d.backend,
            "pixels": d.pixels,
            "over_tol": d.over_tol,
            "max_delta": d.max_delta,
            "mean_delta": d.mean_delta,
        })).collect::<Vec<_>>(),
        "frames_over_limit": bad,
        "qml_errors": errors.len(),
    });
    std::fs::write(
        out.join("report.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "gpushot: {} frame comparisons, {bad} over the limit",
        diffs.len()
    );
    if bad > 0 {
        std::process::exit(3);
    }
}
