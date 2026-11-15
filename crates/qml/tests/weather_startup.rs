//! The tree's Weather singleton at shell startup. Qt creates a QML singleton on first use, so
//! Weather (named only by the weather widget, behind `Config.options.bar.weather.enable`)
//! starts after Config has read config.json: with `enableGPS: false` it never starts the GPS
//! service, and its first fetch is for the configured city, not the defaults.
mod event_loop;

use qml::runtime::Runtime;
use std::path::PathBuf;

fn ri() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"))
}

const MAIN: &str = "import QtQuick
import Rustshell
import ri.modules.common
Item {
    id: root
    property int screens: Rustshell.screens.length
    property var probe: l.item
    Loader { id: l; active: Config.options.bar.weather.enable; sourceComponent: WeatherProbe {} }
}
";

const PROBE: &str = "import QtQuick
import ri.services
Item {
    property bool gps: Weather.gpsActive
    property string city: Weather.city
}
";

thread_local! {
    /// The console lines of this thread's runtime.
    static CONSOLE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[test]
fn weather_waits_for_config() {
    let _ = qml::builtins::CONSOLE_SINK.set(|_, line| {
        CONSOLE.with_borrow_mut(|c| c.push(line.to_string()));
    });
    let test = concat!(module_path!(), "::weather_waits_for_config");
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        let cfg = home.path().join(".config/impulse-shell");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("config.json"),
            r#"{"bar": {"weather": {"enable": true, "enableGPS": false, "city": "Paris"}}}"#,
        )
        .unwrap();
        let dir = home.path().join("qml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("main.qml"), MAIN).unwrap();
        std::fs::write(dir.join("WeatherProbe.qml"), PROBE).unwrap();
        // The fetch must not reach wttr.in.
        let proxy = ["http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY"]
            .map(|k| (k, std::ffi::OsString::from("http://127.0.0.1:9")));
        let vars = testenv::home_vars(home.path()).into_iter().chain(proxy);
        testenv::run_in_own_process(test, vars);
        return;
    }
    let dir = PathBuf::from(std::env::var_os("HOME").unwrap()).join("qml");

    let mut rt = Runtime::new(Some(ri()));
    let mut lp = event_loop::Loop::new(&mut rt);
    let dirs = ["modules/common", "modules/common/functions", "services"].map(|d| ri().join(d));
    rt.load_singletons_lazy(&dirs);
    // As the shell: outputs are published before anything has named `Rustshell`.
    rt.set_screens(&[("Virtual-1".into(), 0, 0, 1920, 1080)]);
    let r = rt.load_file(&dir.join("main.qml")).unwrap();
    rt.settle();
    assert_eq!(rt.get(r, "screens").to_js_string(), "1");
    assert!(
        !rt.world.borrow().singletons.contains_key("Weather"),
        "Weather created before its widget"
    );

    let mut probe = None;
    assert!(
        lp.until(&mut rt, |rt| match rt.get(r, "probe") {
            qml::value::Value::Item(p) => {
                probe = Some(p);
                true
            }
            _ => false,
        }),
        "weather widget never loaded: {:?}",
        rt.errors()
    );
    let probe = probe.unwrap();
    // Everything the widget's start set off at this instant, the first fetch included.
    lp.drain(&mut rt);

    assert!(!rt.get(probe, "gps").truthy());
    assert_eq!(rt.get(probe, "city").to_js_string(), "Paris");
    let gps: Vec<String> =
        CONSOLE.with_borrow(|c| c.iter().filter(|l| l.contains("GPS")).cloned().collect());
    assert!(gps.is_empty(), "{gps:?}");
    let w = rt.world.borrow();
    let weather = w.singletons["Weather"];
    let fetcher = (0..w.items.len())
        .filter(|&i| w.items[i].parent == Some(weather))
        .find(|&i| w.ctxs.iter().any(|c| c.ids.get("fetcher") == Some(&i)))
        .expect("fetcher");
    let place = w.items[fetcher]
        .props
        .get("place")
        .unwrap()
        .value
        .to_js_string();
    assert_eq!(place, "Paris");
}
