//! The password a PamContext conversation answers with is wiped before the helper frees it: in
//! the helper (this test binary, `qml::pam::serve`), an allocator that moves every reallocated
//! block looks at each block freed during the conversation, and ends the helper at once if one
//! still holds the password. The conversation then fails instead of succeeding.

use qml::runtime::Runtime;
use qml::value::Value;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, Ordering};
use testenv::Wakeups;

const PASSWORD: &[u8] = b"pam-secret-wipe-0123456789-long-password";
/// Exit status of a helper that freed a block holding the password.
const LEAKED: i32 = 97;

/// Set in the helper only, before the conversation starts.
static ARMED: AtomicBool = AtomicBool::new(false);

/// [`System`] with `GlobalAlloc`'s default `realloc` (allocate, copy, free): every grown block
/// moves, so a grown buffer leaves its old copy to `dealloc`, which checks it.
struct Watch;

// SAFETY: every call goes to `System` with the caller's arguments; `dealloc` only reads the
// block it is handed, which is live and `l.size()` bytes long until System frees it.
unsafe impl GlobalAlloc for Watch {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: the caller's contract, passed on.
        unsafe { System.alloc(l) }
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        // SAFETY: the caller's contract, passed on.
        unsafe { System.alloc_zeroed(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if ARMED.load(Ordering::Relaxed) {
            // SAFETY: `p` is a live block of `l.size()` bytes (the caller's contract).
            let b = unsafe { std::slice::from_raw_parts(p, l.size()) };
            if b.windows(PASSWORD.len()).any(|w| w == PASSWORD) {
                let m = b"pam_secret_wipe: a freed block still holds the password\n";
                // SAFETY: write(2) takes a descriptor and `m`, live for the call; allocates nothing.
                unsafe { libc::write(2, m.as_ptr().cast(), m.len()) };
                // SAFETY: _exit(2) has no preconditions; it ends the helper.
                unsafe { libc::_exit(LEAKED) };
            }
        }
        // SAFETY: the caller's contract, passed on.
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static WATCH: Watch = Watch;

const HELPER: &str = concat!(module_path!(), "::conversation_helper");

/// The conversation helper when [`helper_command`] runs it, watching what it frees; a no-op
/// test otherwise.
#[test]
fn conversation_helper() {
    if testenv::in_own_process(HELPER) {
        ARMED.store(true, Ordering::Relaxed);
        qml::pam::serve();
    }
}

fn helper_command() -> std::process::Command {
    let mut c = testenv::own_process_command(HELPER);
    c.stdout(std::process::Stdio::null());
    c
}

/// pam_exec asks for the password (`expose_authtok`) and hands it to a script that accepts only
/// [`PASSWORD`]: the helper reads the response, makes it a C string for PAM and wipes it.
#[test]
fn freed_response_holds_no_password() {
    let d = tempfile::tempdir().unwrap();
    let check = d.path().join("check.sh");
    let pw = std::str::from_utf8(PASSWORD).unwrap();
    testenv::install_script(
        &check,
        &format!("#!/bin/sh\nread -r p\n[ \"$p\" = '{pw}' ]\n"),
    );
    std::fs::write(
        d.path().join("t"),
        format!(
            "auth required pam_exec.so expose_authtok quiet {}\n",
            check.display()
        ),
    )
    .unwrap();
    qml::pam::enable(helper_command);
    let mut rt = Runtime::new(None);
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let r = rt
        .load_str(&format!(
            "import QtQuick\nimport Rustshell.Services.Pam\nItem {{ id: r\n\
             property string log: ''; property bool go: false\n\
             onGoChanged: pam.start()\n\
             PamContext {{ id: pam; configDirectory: '{}'; config: 't'\n\
               onPamMessage: {{ r.log += 'M:' + this.message + ';'; if (this.responseRequired) this.respond('{pw}') }}\n\
               onCompleted: result => r.log += 'done:' + result\n\
               onError: error => r.log += 'err:' + error }} }}",
            d.path().display()
        ))
        .unwrap();
    rt.settle();
    rt.world.borrow_mut().write(r, "go", Value::Bool(true));
    rt.settle();
    eprintln!("waiting for the conversation to end");
    loop {
        rt.poll_native();
        rt.settle();
        let log = rt.get(r, "log").to_js_string();
        if log.contains("done:") || log.contains("err:") {
            assert_eq!(log, "M:Password: ;done:0");
            return;
        }
        if wake.wait(ep, None).fd {
            rt.process_fd_ready();
        }
    }
}
