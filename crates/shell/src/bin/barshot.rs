//! Render the original bar offscreen: barshot RI_ROOT OUT.png
use shell::scene;

fn main() {
    qml::debug::init_log();
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(&a[1]);
    let mut rt = qml::runtime::Runtime::new(Some(root.clone()));
    rt.load_singletons(&[
        root.join("modules/common"),
        root.join("modules/common/functions"),
        root.join("modules/common/utils"),
        root.join("services"),
        root.clone(),
    ]);
    rt.set_screens(&[("Virtual-1".into(), 0, 0, 1920, 1080)]);
    rt.load_file(&root.join("modules/rusty/bar/Bar.qml"))
        .expect("load bar");
    for _ in 0..5 {
        if !rt.poll_native() {
            break;
        }
    }
    let wins = scene::find_windows(&rt, "rustshell:bar");
    let win = *wins.first().expect("no bar window");
    let h = rt.get(win, "implicitHeight").to_number();
    let mut fonts = scene::Fonts::default();
    let t = std::time::Instant::now();
    let mut now = 0.0;
    for _ in 0..3 {
        scene::layout_window(&mut rt, &mut fonts, win, [1920.0, h]);
        for _ in 0..600 {
            now += 16.0;
            if !rt.tick(now) {
                break;
            }
        }
    }
    scene::layout_window(&mut rt, &mut fonts, win, [1920.0, h]);
    let tl = t.elapsed();
    let mut cv = qsg::raster::Canvas::new(1920, h as usize, [0, 0, 0, 0]);
    scene::paint_window(&mut rt, &mut fonts, win, &mut scene::Target::Cpu(&mut cv));
    println!(
        "bar {}x{h} layout {tl:?} total {:?} errors {}",
        1920,
        t.elapsed(),
        rt.errors().len()
    );
    for e in rt.errors() {
        println!("  err {e}");
    }
    if std::env::var("GEOM").is_ok() {
        fn walk(rt: &qml::runtime::Runtime, i: usize, d: usize) {
            let (t, ch, id) = {
                let w = rt.world.borrow();
                let it = &w.items[i];
                (
                    qml::runtime::join_syms(&it.types, "/"),
                    it.children.clone(),
                    w.ctxs
                        .iter()
                        .flat_map(|c| c.ids.iter())
                        .find(|(_, v)| **v == i)
                        .map(|(k, _)| k.clone())
                        .unwrap_or_default(),
                )
            };
            let g = |n: &str| rt.get(i, n).to_number();
            println!(
                "{:w$}#{i} {t} {id:?} x={} y={} w={} h={} iw={} vis={:?}",
                "",
                g("x"),
                g("y"),
                g("width"),
                g("height"),
                g("implicitWidth"),
                rt.get(i, "visible"),
                w = d * 2
            );
            if d < 14 {
                for c in ch {
                    walk(rt, c, d + 1);
                }
            }
        }
        walk(&rt, win, 0);
    }
    cv.save_png(&a[2]).unwrap();
}
