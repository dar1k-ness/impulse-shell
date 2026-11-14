//! `Live::gsettings_set`/`gsettings_get` against a real dconf-service on a private session bus,
//! compared with what switchwall.sh's `gsettings set`/`gsettings get` leave behind.
use colorgen::live::Live;
use colorgen::sys::Sys;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

const IFACE: &str = "org.gnome.desktop.interface";
const DARK: [(&str, &str); 2] = [
    ("color-scheme", "prefer-dark"),
    ("gtk-theme", "adw-gtk3-dark"),
];
const LIGHT: [(&str, &str); 2] = [("color-scheme", "prefer-light"), ("gtk-theme", "adw-gtk3")];

struct Bus(Child, String);

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A session bus for a dconf-service with `config` as its XDG_CONFIG_HOME, listening on
/// `address` (`None`: one the daemon picks).
fn bus(config: &Path, address: Option<&str>) -> Bus {
    let mut c = Command::new("dbus-daemon");
    c.args(["--session", "--nofork", "--print-address"]);
    if let Some(a) = address {
        c.arg(format!("--address={a}"));
    }
    let mut c = c
        .env("XDG_CONFIG_HOME", config)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut addr = String::new();
    BufReader::new(c.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    Bus(c, addr.trim().to_string())
}

fn gsettings(bus: &Bus, config: &Path, args: &[&str]) -> String {
    let o = Command::new("gsettings")
        .args(args)
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.1)
        .env("XDG_CONFIG_HOME", config)
        .env("GSETTINGS_BACKEND", "dconf")
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn dump(bus: &Bus, config: &Path) -> String {
    let o = Command::new("dconf")
        .args(["dump", "/"])
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.1)
        .env("XDG_CONFIG_HOME", config)
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn dconf_switch_matches_gsettings() {
    let schema = Command::new("gsettings")
        .args(["list-keys", IFACE])
        .output();
    if !testenv::require_all(&["dbus-daemon", "dconf", "gsettings"]) {
        return;
    }
    if !schema.is_ok_and(|o| o.status.success())
        || !Path::new("/usr/share/dbus-1/services/ca.desrt.dconf.service").exists()
    {
        return testenv::equiv_missing("dconf-service and the GNOME interface schema");
    }
    // Live reads the environment per call: the native side runs in a process of its own with
    // its bus and config directory set there.
    const TEST: &str = concat!(module_path!(), "::dconf_switch_matches_gsettings");
    if !testenv::in_own_process(TEST) {
        let tmp = tempfile::tempdir().unwrap();
        let rs_cfg = tmp.path().join("rs");
        std::fs::create_dir_all(&rs_cfg).unwrap();
        let addr = format!("unix:path={}", tmp.path().join("rs-bus").display());
        testenv::run_in_own_process(
            TEST,
            [
                ("DBUS_SESSION_BUS_ADDRESS", addr.into()),
                ("XDG_CONFIG_HOME", rs_cfg.into_os_string()),
            ],
        );
        return;
    }
    let rs_cfg = std::path::PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").unwrap());
    let ref_cfg = rs_cfg.with_file_name("ref");
    std::fs::create_dir_all(&ref_cfg).unwrap();
    let rs_addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let (ref_bus, rs_bus) = (bus(&ref_cfg, None), bus(&rs_cfg, Some(&rs_addr)));
    let live = Live;
    assert_eq!(live.gsettings_get("color-scheme"), None);
    for (step, keys) in [DARK, LIGHT, DARK, LIGHT].iter().enumerate() {
        for (k, v) in keys {
            gsettings(&ref_bus, &ref_cfg, &["set", IFACE, k, v]);
            live.gsettings_set(k, v);
        }
        assert_eq!(
            dump(&rs_bus, &rs_cfg),
            dump(&ref_bus, &ref_cfg),
            "step {step}"
        );
        for (k, v) in keys {
            let want = gsettings(&ref_bus, &ref_cfg, &["get", IFACE, k]);
            assert_eq!(want, format!("'{v}'"), "step {step}");
            assert_eq!(
                gsettings(&rs_bus, &rs_cfg, &["get", IFACE, k]),
                want,
                "step {step}"
            );
            assert_eq!(live.gsettings_get(k).as_deref(), Some(*v), "step {step}");
        }
    }
    drop((ref_bus, rs_bus));
}
