//! The original's Hyprsunset at startup in automatic mode, inside the night window, with hyprsunset
//! already at the night temperature: the Night Light toggle must read Active, as in Qt.

use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use testenv::Wakeups;

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// In its own process (`testenv::run_in_own_process`), whose `XDG_RUNTIME_DIR` and
/// `HYPRLAND_INSTANCE_SIGNATURE` name the fake hyprsunset's socket and whose
/// `XDG_CONFIG_HOME` holds the config.
#[test]
fn night_light_active_at_startup_in_window() {
    const TEST: &str = concat!(module_path!(), "::night_light_active_at_startup_in_window");
    if !testenv::in_own_process(TEST) {
        let base = tempfile::tempdir().unwrap();
        let cfg = base.path().join("config");
        std::fs::create_dir_all(base.path().join("hypr/TEST")).unwrap();
        std::fs::create_dir_all(cfg.join("impulse-shell")).unwrap();
        std::fs::write(
            cfg.join("impulse-shell/config.json"),
            r#"{"light":{"night":{"automatic":true,"from":"19:00","to":"06:30","colorTemperature":5000}}}"#,
        )
        .unwrap();
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_RUNTIME_DIR", base.path().as_os_str()),
                ("XDG_CONFIG_HOME", cfg.as_os_str()),
                ("HYPRLAND_INSTANCE_SIGNATURE", std::ffi::OsStr::new("TEST")),
            ],
        );
        return;
    }
    let dir = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join("hypr/TEST");
    let reqs = Arc::new(Mutex::new(Vec::<String>::new()));
    let l = UnixListener::bind(dir.join(".hyprsunset.sock")).unwrap();
    let log = reqs.clone();
    let wake = Wakeups::new();
    let ping = wake.ping();
    std::thread::spawn(move || {
        let mut temp = "5000".to_string();
        // Like hyprsunset's IPCSocket: one connection at a time, replies to every request
        // on it and closes only once the client has.
        for mut s in l.incoming().flatten() {
            let mut buf = [0u8; 1024];
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
                log.lock().unwrap().push(cmd.clone());
                ping();
                let r = match cmd.strip_prefix("temperature") {
                    Some("") => temp.clone(),
                    Some(t) => {
                        temp = t.trim().to_string();
                        "ok".into()
                    }
                    None => "ok".into(),
                };
                let _ = s.write_all(r.as_bytes());
            }
        }
    });
    qml::native::set_spawn_hook(Some(Box::new(|_: &[String]| Some(String::new()))));
    // The engine's clock at the window's first minute, either side of midnight and its last
    // minute: one startup at each.
    for (hour, minute) in [(19, 0), (23, 59), (0, 0), (6, 30)] {
        qml::builtins::set_clock(Some(testenv::local_time_ms(hour, minute)));
        reqs.lock().unwrap().clear();
        let mut rt = Runtime::new(Some(ri()));
        rt.world.borrow().waker.set_ping(wake.ping());
        rt.load_singletons(&[
            ri().join("services"),
            ri().join("modules/common"),
            ri().join("modules/common/functions"),
        ]);
        let r = rt
            .load_str(
                "import QtQuick\nimport ri.services\nItem { property var s: Hyprsunset\n\
                 property var seen: []\n\
                 Connections { target: Hyprsunset\n\
                 function onTemperatureActiveChanged() { seen.push(Hyprsunset.temperatureActive) } }\n\
                 Component.onCompleted: Hyprsunset.fetchState() }",
            )
            .unwrap();
        // Until hyprsunset answered the state query and nothing the service started is still
        // out: no `hyprctl` call in flight, no detached start pending.
        eprintln!("{hour:02}:{minute:02}: waiting for the state query and the startup calls");
        let ep = rt.process_fd();
        loop {
            workpool::wait_idle();
            let rev = rt.revision();
            rt.poll_native();
            rt.settle();
            if rt.revision() == rev
                && qml::hyprctl::in_flight(&rt.world.borrow()) == 0
                && reqs.lock().unwrap().iter().any(|c| c == "temperature")
            {
                break;
            }
            if rev == rt.revision() && wake.wait(ep, None).fd {
                rt.process_fd_ready();
            }
        }
        let Value::Item(s) = rt.get(r, "s") else {
            panic!("no s: {:?}", rt.errors())
        };
        let seen = rt.get(r, "seen").to_js_string();
        assert!(
            rt.get(s, "temperatureActive").truthy(),
            "at {hour:02}:{minute:02}: seen {seen}, requests {:?}",
            reqs.lock().unwrap()
        );
    }
    qml::builtins::set_clock(None);
}
