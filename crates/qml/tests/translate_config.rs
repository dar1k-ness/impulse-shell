//! The translation's `.language.ui` edit goes through the running shell's Config: written to
//! config.json directly (the original `jq … > f.tmp && mv`), Config's pending delayed save wrote
//! its stale `language.ui` back over the new locale.
mod event_loop;

use qml::runtime::Runtime;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

#[test]
fn translation_locale_survives_pending_config_save() {
    const TEST: &str = concat!(
        module_path!(),
        "::translation_locale_survives_pending_config_save"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let cfg_dir = home.join(".config/rusty-impulse");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(
            cfg_dir.join("config.json"),
            r#"{
    "language": {
        "ui": "auto"
    },
    "bar": {
        "weather": {
            "city": "Moscow"
        }
    }
}
"#,
        )
        .unwrap();
        testenv::run_in_own_process_env(
            TEST,
            testenv::home_vars(home),
            &["DBUS_SESSION_BUS_ADDRESS"],
        );
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let cfg_dir = home.join(".config/rusty-impulse");
    let cfg = cfg_dir.join("config.json");
    let mut rt = Runtime::new(Some(ri()));
    let mut lp = event_loop::Loop::new(&mut rt);
    let keep = ["Directories", "Config", "FileUtils", "StringUtils"];
    let dirs = ["modules/common", "modules/common/functions"].map(|d| ri().join(d));
    rt.load_singletons_where(&dirs, &|n| keep.contains(&n));
    let f = home.join("main.qml");
    std::fs::write(
        &f,
        "import QtQuick\nimport ri.modules.common\nItem { property string ui: Config.options.language.ui; property string city: Config.options.bar.weather.city; function edit() { Config.options.bar.weather.city = \"Paris\" } }\n",
    )
    .unwrap();
    let root = rt.load_file(&f).unwrap();
    // Config's read and write delays run on the loop's virtual clock.
    lp.advance(&mut rt, 300.0);
    // A user edit whose save is still pending in Config's write timer...
    rt.call_method(root, "edit", vec![]).unwrap().unwrap();
    // ...when the translation job (on its own thread) finishes and switches the UI locale.
    let script = home.join("rustshell/rusty-impulse/scripts/ai/gemini-translate.sh");
    let texts = home.join("rustshell/rusty-impulse/translations");
    std::fs::create_dir_all(&texts).unwrap();
    std::fs::write(texts.join("en_US.json"), "{\"Hello\": \"Hello\"}").unwrap();
    let gemini = translation_server();
    let (h, xdg) = (
        home.to_string_lossy().into_owned(),
        home.join(".config").to_string_lossy().into_owned(),
    );
    std::thread::spawn(move || {
        let env = |k: &str| match k {
            "HOME" => Some(h.clone()),
            "XDG_CONFIG_HOME" => Some(xdg.clone()),
            _ => None,
        };
        qml::jobs_ai::translate::run(
            &script.to_string_lossy(),
            &["ru_RU".to_string()],
            &env,
            &gemini,
            &mut |_| {},
            &mut |_| {},
        )
    })
    .join()
    .unwrap();
    lp.advance(&mut rt, 600.0);
    let read = |rt: &mut Runtime, k: &str| {
        rt.world
            .borrow_mut()
            .read(root, k)
            .unwrap()
            .to_js_string()
            .to_string()
    };
    assert_eq!(read(&mut rt, "ui"), "ru_RU");
    assert_eq!(read(&mut rt, "city"), "Paris");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
    assert_eq!(v["language"]["ui"], "ru_RU");
    assert_eq!(v["bar"]["weather"]["city"], "Paris");
    assert!(!cfg_dir.join("config.json.tmp").exists());
}

/// A Gemini endpoint that answers one request with a valid translation (a JSON object of
/// strings), so the job switches the locale.
fn translation_server() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        let (c, _) = l.accept().unwrap();
        let mut r = BufReader::new(c.try_clone().unwrap());
        let mut len = 0;
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':')
                && k.eq_ignore_ascii_case("content-length")
            {
                len = v.trim().parse().unwrap();
            }
        }
        r.read_exact(&mut vec![0; len]).unwrap();
        let body = r#"{"candidates": [{"content": {"parts": [{"text": "{\"Settings\": \"Настройки\"}"}]}}]}"#;
        let mut c = c;
        write!(
            c,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    base
}
