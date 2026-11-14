//! A theme switch through the real live client against a Hyprland request socket: the eval
//! is answered "ok" only after the compositor has finished applying it, and that answer is
//! what decides that no `reload` follows and autoreload is resumed.

use colorgen::hyprlive::{eval_request, write};
use colorgen::live::Live;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;

const PAUSE: &str = "eval hl.config({ misc = { disable_autoreload = true } })";
const RESUME: &str = "eval hl.config({ misc = { disable_autoreload = false } })";
const NEW: &str = "hl.config({ general = { col = { active_border = \"rgba(11223377)\" } } })\n";

/// The live client finds the compositor through `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE`:
/// the test runs in its own process with both pointing at a fresh directory.
#[test]
fn switch_goes_through_the_shared_hyprland_client() {
    const TEST: &str = concat!(
        module_path!(),
        "::switch_goes_through_the_shared_hyprland_client"
    );
    if !testenv::in_own_process(TEST) {
        let rt = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(rt.path().join("hypr/sig")).unwrap();
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_RUNTIME_DIR", rt.path().as_os_str()),
                ("HYPRLAND_INSTANCE_SIGNATURE", std::ffi::OsStr::new("sig")),
            ],
        );
        return;
    }
    let rt = std::env::var_os("XDG_RUNTIME_DIR").unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let inst = std::path::Path::new(&rt).join("hypr/sig");
    let l = UnixListener::bind(inst.join(".socket.sock")).unwrap();
    let out = cfg.path().join("hypr/hyprland/colors.lua");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(&out, "old\n").unwrap();
    let eval = eval_request(&out, NEW);

    // Hyprland closes each connection after its reply; the reply comes in pieces, as a large
    // one does, and only the closed connection marks its end.
    let srv = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for c in l.incoming().take(4) {
            let mut c = c.unwrap();
            let mut buf = vec![0u8; 1 << 16];
            let n = c.read(&mut buf).unwrap();
            let cmd = String::from_utf8_lossy(&buf[..n]).into_owned();
            let reply = if cmd == "j/getoption misc:disable_autoreload" {
                r#"{"option": "misc:disable_autoreload", "bool": false, "set": false}"#
            } else {
                "ok"
            };
            let (a, b) = reply.split_at(1);
            c.write_all(a.as_bytes()).unwrap();
            c.write_all(b.as_bytes()).unwrap();
            requests.push(cmd);
        }
        requests
    });

    write(&Live, &out, NEW).unwrap();

    assert_eq!(
        srv.join().unwrap(),
        ["j/getoption misc:disable_autoreload", PAUSE, &eval, RESUME],
        "paused, applied once by eval, resumed; no reload"
    );
    assert_eq!(std::fs::read_to_string(&out).unwrap(), NEW);
}
