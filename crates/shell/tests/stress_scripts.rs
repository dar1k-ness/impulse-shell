//! The scripts of `tools/stress`, run without the guest: `$HOME` is a directory holding them
//! as `~/h`, and `hyprctl`, `pkill` and the other programs they call are scripts. The shell
//! under test is a process that does nothing; called as its own IPC client
//! (`rusty-impulse -c rusty-impulse ipc call ...`, which is how the scripts reach it) it answers.
//!
//! What is pinned: a scenario that could not do its work does not end `OK` (a program it
//! needs is missing, a click was not made, a lock round did not unlock), and `matrix.sh`
//! records a shell that left during a scenario as `CRASH` and runs the scenarios after it.
//! (It stopped there: with no shell, `cputicks` read `/proc//stat`, and the arithmetic on
//! those lines ended the loop. A `tap` that failed 160 times left the scenario `OK`.)

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn script(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The program `rusty-impulse`: its IPC client answers every call (after `on_call`, bash run
/// with the call's arguments); started with no arguments it is the shell, which has nothing to
/// do.
fn shell(on_call: &str) -> String {
    format!(
        "#!/bin/bash\nif [ \"$3\" = ipc ]; then\n{on_call}exit 0\nfi\n\
         flock -u 8 2>/dev/null\nexec 8>&- 9>&-\nexec tail -f /dev/null\n"
    )
}

struct Guest {
    tmp: tempfile::TempDir,
}

impl Guest {
    fn new() -> Guest {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(home.join("h")).unwrap();
        std::fs::create_dir(&bin).unwrap();
        let stress = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/stress");
        for entry in std::fs::read_dir(stress).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), home.join("h").join(entry.file_name())).unwrap();
        }
        // What tools/stress/setup.sh writes: the environment of the Hyprland session.
        std::fs::write(home.join("henv"), "").unwrap();
        // Hyprland starts the shell (launch.sh); the shell answers every IPC call. What it
        // starts holds two locks, both until it has exited and been waited for: `started`,
        // and `busy`, which a program gives up (`flock -u 8`) where it has nothing to do
        // until something else happens.
        script(
            &bin.join("hyprctl"),
            r#"#!/bin/bash
case "$2" in
'hl.dsp.exec_cmd("'*)
    cmd=${2#*\"}; cmd=${cmd%\"*}
    exec 8>>"${0%/*}/busy" 9>>"${0%/*}/started"
    flock -s 8; flock -s 9
    (bash -c "$cmd" & wait) >/dev/null 2>&1 </dev/null &
    ;;
esac
"#,
        );
        // Quickshell's `qs` does not find the native shell: a script that asked it would wait.
        script(&bin.join("qs"), "#!/bin/bash\nexit 255\n");
        // The one shell there is: the process of $RUN/pid. (The real pkill would look for
        // every `rusty-impulse` of the machine.)
        script(
            &bin.join("pkill"),
            "#!/bin/bash\np=$(cat \"$RUN/pid\" 2>/dev/null)\n[ -n \"$p\" ] && [ -d /proc/$p ] && kill \"$p\"\n",
        );
        // The shell is all Hyprland started: it is gone when nothing holds `started`.
        script(
            &bin.join("pidwait"),
            "#!/bin/bash\nexec flock \"${0%/*}/started\" true\n",
        );
        // The scripts wait in seconds for programs that here are done at once: the wait is
        // over when what Hyprland started has exited or has nothing to do (`busy` is free).
        script(
            &bin.join("sleep"),
            "#!/bin/bash\nexec flock \"${0%/*}/busy\" true\n",
        );
        for name in ["coredumpctl", "journalctl"] {
            script(&bin.join(name), "#!/bin/bash\nexit 0\n");
        }
        // matrix.sh's two samplers look at the shell every few seconds for as long as it
        // runs: like the shell, processes that do nothing.
        for name in ["sampler.sh", "ipcprobe.sh"] {
            script(
                &home.join("h").join(name),
                "#!/bin/bash\nexec tail -f /dev/null\n",
            );
        }
        script(&bin.join("rusty-impulse"), &shell(""));
        Guest { tmp }
    }

    fn home(&self) -> PathBuf {
        self.tmp.path().join("home")
    }

    fn bin(&self) -> PathBuf {
        self.tmp.path().join("bin")
    }

    /// A scenario `~/h/NAME.sh`.
    fn scenario(&self, name: &str, body: &str) {
        script(
            &self.home().join(format!("h/{name}.sh")),
            &format!("#!/bin/bash\nRUN=$1; . ~/h/common.sh\n{body}\n"),
        );
    }

    /// `~/h/NAME ARGS` with the scripts of `bin` before the programs of the machine.
    fn run(&self, name: &str, args: &[&str], env: &[(&str, &str)]) -> Output {
        let path = std::env::var("PATH").unwrap();
        Command::new("bash")
            .arg(self.home().join("h").join(name))
            .args(args)
            .env("HOME", self.home())
            .env("PATH", format!("{}:{path}", self.bin().display()))
            .env("BIN", self.bin().join("rusty-impulse"))
            .envs(env.iter().copied())
            .output()
            .expect("bash")
    }

    /// The directory of the one run `matrix.sh` made.
    fn run_dir(&self) -> PathBuf {
        let mut runs = std::fs::read_dir(self.home().join("stress")).unwrap();
        let run = runs.next().unwrap().unwrap().path();
        assert!(runs.next().is_none());
        run
    }
}

impl Drop for Guest {
    /// `matrix.sh` leaves the shell running; a test that failed may leave a `pkexec` whose
    /// dialog nobody answered (its `pkexec.pid` is there while it waits).
    fn drop(&mut self) {
        let mut pids = vec![self.tmp.path().join("pkexec.pid")];
        if let Ok(dir) = std::fs::read_dir(self.home().join("stress")) {
            pids.extend(dir.map(|run| run.unwrap().path().join("pid")));
        }
        for pid in pids {
            let pid = std::fs::read_to_string(pid);
            if let Some(pid) = pid.ok().and_then(|p| p.trim().parse::<i32>().ok()) {
                // SAFETY: kill takes two integers and touches no memory of this process.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        }
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_matrix_records_a_shell_that_left_as_crash_and_runs_the_rest() {
    let guest = Guest::new();
    guest.scenario("good", "true");
    // The shell is killed and is gone when the scenario ends.
    guest.scenario("dies", "kill -9 $(spid); pidwait -F $RUN/pid");
    guest.scenario("after", "true");
    guest.scenario("bad", "exit 3");
    guest.scenario("lacks", "need bash no-such-program\necho reached");
    let out = guest.run(
        "matrix.sh",
        &["native", "good", "dies", "after", "bad", "lacks"],
        &[],
    );
    let err = text(&out.stderr);
    let run = guest.run_dir();
    let results = std::fs::read_to_string(run.join("results.txt")).unwrap();
    assert_eq!(
        results, "good OK\ndies CRASH\nafter OK\nbad FAIL\nlacks FAIL\nDONE\n",
        "stderr: {err}"
    );
    assert_eq!(out.status.code(), Some(1), "stderr: {err}");
    assert!(
        err.contains("lacks.sh: no-such-program is not installed"),
        "{err}"
    );
    assert!(!text(&out.stdout).contains("reached"));
    assert!(!err.contains("syntax error"), "{err}");
    let events = std::fs::read_to_string(run.join("events.log")).unwrap();
    assert!(events.contains("== dies end rc=0 result=CRASH"), "{events}");
    // `after` ran on a shell started for it.
    let pid_of = |scenario: &str| {
        let line = events
            .lines()
            .find(|l| l.starts_with(&format!("== {scenario} start pid=")))
            .unwrap_or_else(|| panic!("{events}"));
        line.rsplit('=').next().unwrap().to_string()
    };
    assert!(!pid_of("after").is_empty());
    assert_ne!(pid_of("after"), pid_of("dies"));
}

#[test]
fn the_matrix_exits_0_when_every_scenario_ended_ok() {
    let guest = Guest::new();
    guest.scenario("good", "true");
    guest.scenario("after", "true");
    let out = guest.run("matrix.sh", &["native", "good", "after"], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let results = std::fs::read_to_string(guest.run_dir().join("results.txt")).unwrap();
    assert_eq!(results, "good OK\nafter OK\nDONE\n");
}

/// `tap X Y` is `ptr.py click X Y` of `tools/parity`; when that fails the scenario ends.
#[test]
fn a_tap_is_a_click_of_ptr_py_and_one_not_made_ends_the_scenario() {
    let guest = Guest::new();
    let run = guest.tmp.path().join("run");
    std::fs::create_dir(&run).unwrap();
    let run = run.to_str().unwrap();
    guest.scenario("taps", "tap 712 886; tap 10 20; echo reached");
    let ptr = guest.tmp.path().join("ptr.py");
    let calls = guest.tmp.path().join("calls");
    std::fs::write(
        &ptr,
        format!(
            "import sys\nopen({:?}, 'a').write(' '.join(sys.argv[1:]) + '\\n')\n",
            calls.to_str().unwrap()
        ),
    )
    .unwrap();
    let out = guest.run("taps.sh", &[run], &[("PTR", ptr.to_str().unwrap())]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        std::fs::read_to_string(&calls).unwrap(),
        "click 712 886\nclick 10 20\n"
    );

    // No python-evdev, no /dev/uinput: ptr.py exits with an error.
    std::fs::write(&ptr, "import sys\nsys.exit('no uinput')\n").unwrap();
    let out = guest.run("taps.sh", &[run], &[("PTR", ptr.to_str().unwrap())]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("taps.sh: tap 712 886:"),
        "{out:?}"
    );
    assert!(!text(&out.stdout).contains("reached"));
}

/// `polkit.sh` fails when a request did not end: the click on Cancel that closed no dialog
/// left `pkexec` waiting, and the scenario ended `OK` with half of its rounds stuck.
#[test]
fn the_polkit_scenario_fails_when_a_request_does_not_end() {
    let guest = Guest::new();
    let run = guest.tmp.path().join("run");
    std::fs::create_dir(&run).unwrap();
    let tmp = guest.tmp.path().to_str().unwrap();
    // pkexec has nothing to do until its dialog is answered (a line in the pipe `cancel`);
    // then it says so in the pipe `answered` and is refused. (`Guest::drop` kills one that
    // is still waiting.)
    for pipe in ["cancel", "answered"] {
        let made = Command::new("mkfifo")
            .arg(guest.tmp.path().join(pipe))
            .status()
            .unwrap();
        assert!(made.success());
    }
    script(
        &guest.bin().join("pkexec"),
        &format!(
            "#!/bin/bash\necho $$ >{tmp}/pkexec.pid\nexec 7<>{tmp}/cancel\nflock -u 8\nread -r <&7\nflock -s 8\nrm {tmp}/pkexec.pid\n: >{tmp}/answered\nexit 126\n"
        ),
    );
    script(
        &guest.bin().join("pkill"),
        &format!("#!/bin/bash\nkill $(cat {tmp}/pkexec.pid) 2>/dev/null\nrm -f {tmp}/pkexec.pid\n"),
    );
    script(&guest.bin().join("faillock"), "#!/bin/bash\nexit 0\n");
    let ptr = guest.tmp.path().join("ptr.py");
    let env = [("PTR", ptr.to_str().unwrap())];
    let args = [run.to_str().unwrap(), "2"];
    let ended = || {
        let events = std::fs::read_to_string(run.join("events.log")).unwrap();
        let line = events
            .lines()
            .rfind(|l| l.contains(" polkit ended_ok="))
            .unwrap();
        line.split(' ')
            .skip(2)
            .take(2)
            .collect::<Vec<_>>()
            .join(" ")
    };

    // The click answers the dialog (one that is there: the pipe has its reader), and the
    // pointer is done when the dialog took the answer.
    std::fs::write(
        &ptr,
        format!(
            "import os\nos.write(os.open('{tmp}/cancel', os.O_WRONLY | os.O_NONBLOCK), b'\\n')\nopen('{tmp}/answered').read()\n"
        ),
    )
    .unwrap();
    let out = guest.run("polkit.sh", &args, &env);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(ended(), "ended_ok=2/2 stuck=0");

    // The click lands on no dialog (a locked session, a dialog somewhere else).
    std::fs::write(&ptr, "").unwrap();
    let out = guest.run("polkit.sh", &args, &env);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("polkit.sh: 1 of 2 requests did not end"),
        "{out:?}"
    );
    assert_eq!(ended(), "ended_ok=1/2 stuck=1");
}

/// The real tool `tap` runs takes these arguments (it had none of `jiggle`).
#[test]
fn ptr_py_has_the_click_command() {
    let ptr = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/ptr.py");
    let source = std::fs::read_to_string(ptr).unwrap();
    assert!(source.contains("elif cmd == \"click\":"), "{source}");
}

/// `ptr.py` sends its first event once the compositor lists the device, and closes the device
/// once the compositor has answered a request made after the last event. It did both after a
/// pause (0.4 s, 0.1 s): Hyprland busy for longer had not opened the device, or had not read
/// all of it when it was closed, and `move 1300 700` left the pointer at 1299 (the pixel it
/// arrives through) or where it was.
///
/// `evdev` and `hyprctl` here write what they are asked to a file. The device's node is a file
/// `evdev` creates (an open that is not the compositor's, as python-evdev's own); this
/// `hyprctl` opens it, as the compositor taking the device, when asked for the devices a
/// second time, and lists it from then on.
#[test]
fn ptr_py_sends_to_a_device_the_compositor_has_and_closes_it_after_its_answer() {
    let tmp = tempfile::tempdir().unwrap();
    let (bin, seen) = (tmp.path().join("bin"), tmp.path().join("seen"));
    let input = tmp.path().join("input");
    std::fs::create_dir_all(tmp.path().join("py/evdev")).unwrap();
    std::fs::create_dir(&bin).unwrap();
    std::fs::create_dir(&input).unwrap();
    std::fs::write(
        tmp.path().join("py/evdev/__init__.py"),
        r#"import os

def seen(what):
    with open(os.environ["SEEN"], "a") as f:
        f.write(what + "\n")

class AbsInfo:
    def __init__(self, *limits):
        pass

class ecodes:
    EV_KEY, EV_ABS, BTN_LEFT, BTN_RIGHT, ABS_X, ABS_Y = "key", "abs", "left", "right", "x", "y"

class Device:
    path = os.path.join(os.environ["PARITY_INPUT_DIR"], "event7")

class UInput:
    def __init__(self, events, name):
        seen("open")
        self.device = Device()
        with open(self.device.path, "w"), open(os.environ["SEEN"] + ".name", "w") as f:
            f.write(name)

    def write(self, kind, code, value):
        seen(f"{kind} {code} {value}")

    def syn(self):
        seen("syn")

    def close(self):
        seen("close")
"#,
    )
    .unwrap();
    script(
        &bin.join("hyprctl"),
        r#"#!/bin/bash
echo "hyprctl $*" >>"$SEEN"
[ "$*" = "$REFUSE" ] && exit 1
[ "$*" = cursorpos ] && echo '1300, 700' && exit
asked=$(grep -c '^hyprctl devices' "$SEEN")
echo mice:
[ "$asked" -ge 3 ] && printf '\tMouse at 55d0a0:\n\t\t%s\n' "$(cat "$SEEN.name")"
[ "$asked" = 2 ] && : <"$PARITY_INPUT_DIR/event7"
exit 0
"#,
    );
    let ptr = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/ptr.py");
    let run = |refuse: &str, input: &Path| {
        std::fs::write(&seen, "").unwrap();
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        let out = Command::new("python3")
            .arg(&ptr)
            .args(["move", "1300", "700"])
            .env("PYTHONPATH", tmp.path().join("py"))
            .env("PATH", path)
            .env("SEEN", &seen)
            .env("REFUSE", refuse)
            .env("PARITY_INPUT_DIR", input)
            .output()
            .expect("python3");
        (out, std::fs::read_to_string(&seen).unwrap())
    };
    let (out, did) = run("nothing", &input);
    assert!(out.status.success(), "{out:?}");
    // 16 device units a pixel, the middle of the pixel: 1299 and 1300 across, 700 down.
    assert_eq!(
        did.lines().collect::<Vec<_>>(),
        [
            "open",
            "hyprctl devices",
            "hyprctl devices",
            "hyprctl devices",
            "abs x 20792",
            "abs y 11208",
            "syn",
            "abs x 20808",
            "abs y 11208",
            "syn",
            "hyprctl cursorpos",
            "close",
        ]
    );
    // No answer after the last event: the script fails and says why.
    let (out, did) = run("cursorpos", &input);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let why = "ptr.py: the compositor did not answer `hyprctl cursorpos`";
    assert!(text(&out.stderr).contains(why), "{out:?}");
    assert!(!did.contains("close"), "{did}");
    // No directory to see the device being opened in: no device is made.
    let (out, did) = run("nothing", &tmp.path().join("none"));
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let why = "ptr.py: cannot watch the device nodes: No such file or directory";
    assert!(text(&out.stderr).contains(why), "{out:?}");
    assert_eq!(did, "");
}

/// `lock.sh` types the password `tools/parity/setup.sh` and `tools/stress/setup.sh` give the
/// user, starts from no refused password on record, and fails unless every round ended on the
/// frame the screen had before the lock.
#[test]
fn the_lock_scenario_fails_unless_every_round_unlocked() {
    let guest = Guest::new();
    let run = guest.tmp.path().join("run");
    std::fs::create_dir(&run).unwrap();
    let typed = guest.tmp.path().join("typed");
    // What the scenario saw and did, in order.
    let seen = guest.tmp.path().join("seen");
    script(
        &guest.bin().join("grim"),
        &format!(
            "#!/bin/bash\n: >\"$1\"\necho \"grim ${{1##*/}}\" >>{}\n",
            seen.display()
        ),
    );
    script(
        &guest.bin().join("wtype"),
        &format!(
            "#!/bin/bash\necho \"$*\" >>{}\n[ \"$*\" = '-k Return' ] && echo return >>{}\n",
            typed.display(),
            seen.display()
        ),
    );
    script(
        &guest.bin().join("rusty-impulse"),
        &shell(&format!(
            "[ \"$*\" = '-c rusty-impulse ipc call lock activate' ] && echo activate >>{}\n",
            seen.display()
        )),
    );
    // The compositor: it holds the lock from the second question after the shell was asked for
    // one, and no longer at the third question after Return. With `no-lock` there, the shell
    // never locks; with `held`, the lock is held whatever happens.
    let no_lock = guest.tmp.path().join("no-lock");
    script(
        &guest.bin().join("hyprctl"),
        &format!(
            r#"#!/bin/bash
[ "$*" = "-j locked" ] || exit 0
if [ -e {2} ]; then
    printf '{{\n    "locked": true\n}}\n'
    exit 0
fi
last=$(grep -n -e '^activate' -e '^return' {0} | tail -1)
asked=$(tail -n +$((${{last%%:*}} + 1)) {0} | grep -c '^locked')
case ${{last#*:}} in
    activate) [ $asked -ge 1 ] && [ ! -e {1} ] && state=true || state=false ;;
    return) [ $asked -ge 2 ] && state=false || state=true ;;
    *) state=false ;;
esac
echo "locked $state" >>{0}
printf '{{\n    "locked": %s\n}}\n' $state
"#,
            seen.display(),
            no_lock.display(),
            guest.tmp.path().join("held").display()
        ),
    );
    let faillock = guest.tmp.path().join("faillock");
    script(
        &guest.bin().join("faillock"),
        &format!("#!/bin/bash\necho \"$*\" >>{}\n", faillock.display()),
    );
    // ImageMagick: "ABSOLUTE (NORMALISED)" on stderr. The second round stays locked.
    let rounds = guest.tmp.path().join("rounds");
    let compare = |second: &str| {
        script(
            &guest.bin().join("compare"),
            &format!(
                "#!/bin/bash\necho >>{0}\nif [ $(wc -l <{0}) = 2 ]; then echo '{second}' >&2; else echo '0 (0)' >&2; fi\n",
                rounds.display()
            ),
        );
        let _ = std::fs::remove_file(&rounds);
    };
    let log = run.join("shell.log");
    std::fs::write(&log, "").unwrap();
    let env = [("LOG", log.to_str().unwrap())];
    let args = [run.to_str().unwrap(), "3"];

    compare("0 (0)");
    let out = guest.run("lock.sh", &args, &env);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let typed_path = typed;
    let typed = std::fs::read_to_string(&typed_path).unwrap();
    assert_eq!(typed.lines().next(), Some("parity"), "{typed}");
    // The password is typed once the compositor holds the lock, and the frame of a round is
    // taken once it said the lock was gone, however long either took.
    let seen_path = seen;
    let seen = std::fs::read_to_string(&seen_path).unwrap();
    let mut want = vec!["locked false".to_string(), "grim pre.png".to_string()];
    for round in 1..=3 {
        want.extend(
            [
                "activate",
                "locked false",
                "locked true",
                &format!("grim locked{round}.png"),
                "return",
                "locked true",
                "locked true",
                "locked false",
                &format!("grim un{round}.png"),
            ]
            .map(String::from),
        );
    }
    assert_eq!(seen.lines().collect::<Vec<_>>(), want);
    // Passwords refused before the scenario (polkit.sh's cancelled requests count) are forgotten.
    let faillock = std::fs::read_to_string(faillock).unwrap();
    let user = text(&Command::new("id").arg("-un").output().unwrap().stdout);
    assert_eq!(faillock, format!("--user {} --reset\n", user.trim()));

    compare("30000 (0.45)");
    let out = guest.run("lock.sh", &args, &env);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("lock.sh: 2 of 3 rounds unlocked"),
        "{out:?}"
    );

    // The lock is held when the scenario starts (a shell killed with it, say): every frame
    // would be the same one, and no round is run.
    let held = guest.tmp.path().join("held");
    std::fs::write(&held, "").unwrap();
    let out = guest.run("lock.sh", &args, &env);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("lock.sh: the session is locked before the first round"),
        "{out:?}"
    );
    std::fs::remove_file(&held).unwrap();

    // The shell does not lock (it left, say): no password is typed into the desktop, and the
    // round is not one that unlocked, though the screen shows the frame it had before.
    compare("0 (0)");
    std::fs::write(&no_lock, "").unwrap();
    std::fs::remove_file(&typed_path).unwrap();
    let out = guest.run("lock.sh", &[run.to_str().unwrap(), "1"], &env);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("lock.sh: 0 of 1 rounds unlocked"),
        "{out:?}"
    );
    assert!(!typed_path.exists());
    std::fs::remove_file(&no_lock).unwrap();

    // No wtype in the guest: the scenario says so, and does not lock a screen it cannot unlock.
    std::fs::remove_file(guest.bin().join("wtype")).unwrap();
    if Command::new("wtype").arg("-h").output().is_err() {
        let out = guest.run("lock.sh", &args, &env);
        assert_eq!(out.status.code(), Some(1));
        assert!(
            text(&out.stderr).contains("lock.sh: wtype is not installed"),
            "{out:?}"
        );
    }
}
