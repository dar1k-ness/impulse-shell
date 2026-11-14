//! `scripts/videos/record.sh`: save dir given by the caller, focused monitor from the
//! Hyprland socket (no `hyprctl | jq`), default sink from PipeWire (no `pactl`),
//! notifications over D-Bus (no `notify-send`), stop by signal (no `pgrep`/`pkill`).
//! `slurp` (region pick) and `wf-recorder` still run as programs. The shell's QML calls it
//! through `ScreenRecorder`; a Hyprland keybind runs the tree's `record.sh`, which runs
//! [`serve`] in the shell binary.

use super::Io;
use std::process::Stdio;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    pub region: Option<String>,
    pub fullscreen: bool,
    /// The directory the recording is saved in (`Config.options.screenRecord.savePath`);
    /// empty: `~/Videos`.
    pub save_path: String,
    /// Record no sound. The script has no such launch (its `SOUND_FLAG` starts at 1, so
    /// `--sound` changes nothing); the region selector's "record" without sound asks for it.
    pub silent: bool,
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// `hyprctl monitors -j | jq -r '.[] | select(.focused == true) | .name'`.
pub fn focused_monitor() -> String {
    let Some(dir) = crate::hypr::socket_dir() else {
        return String::new();
    };
    let Some(j) = crate::hypr::request(&dir, "j/monitors") else {
        return String::new();
    };
    focused_in(&j)
}

pub fn focused_in(json: &str) -> String {
    let Ok(serde_json::Value::Array(ms)) = serde_json::from_str(json) else {
        return String::new();
    };
    ms.iter()
        .filter(|m| m.get("focused") == Some(&serde_json::Value::Bool(true)))
        .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Running `wf-recorder` processes (`pgrep wf-recorder`: process name match).
pub fn recorders() -> Vec<i32> {
    let me = std::process::id() as i32;
    let mut v = crate::osutil::pgrep("wf-recorder");
    v.retain(|&pid| pid != me);
    v
}

/// `mkdir -p "$RECORDING_DIR"; cd "$RECORDING_DIR" || exit`: the directory the recording is
/// saved in, made when it is not there; `None` when it cannot be one.
fn save_dir(save_path: &str) -> Option<String> {
    let dir = if save_path.is_empty() {
        format!("{}/Videos", home())
    } else {
        save_path.to_owned()
    };
    let _ = std::fs::create_dir_all(&dir);
    std::path::Path::new(&dir).is_dir().then_some(dir)
}

/// The first argument of the shell binary run in the script's place: `rusty-impulse __ri-record
/// [--fullscreen] [--sound] [--region AREA]` is `record.sh` with those arguments
/// ([`serve`]). The tree's `scripts/videos/record.sh`, the file the original's Hyprland keybinds
/// name, runs it.
pub const ARG: &str = "__ri-record";

/// `jq -r .screenRecord.savePath ~/.config/rusty-impulse/config.json`: the save
/// directory the user's config names, empty when it names none.
fn configured_save_path() -> String {
    let file = crate::xdg::config_home().join("rusty-impulse/config.json");
    std::fs::read_to_string(file)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|c| Some(c.get("screenRecord")?.get("savePath")?.as_str()?.to_owned()))
        .unwrap_or_default()
}

/// The script as a program, for a caller outside the shell (a keybind): `args` are the
/// script's, the save directory is the config file's, and sound is recorded whatever the
/// arguments (the script's `SOUND_FLAG` starts at 1). Returns the script's exit status, once
/// the notifications are sent and a recording it started has ended.
pub fn serve(args: &[String]) -> i32 {
    let mut a = Args {
        save_path: configured_save_path(),
        ..Default::default()
    };
    let mut region_missing = false;
    for (i, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "--region" => match args.get(i + 1) {
                Some(r) => a.region = Some(r.clone()),
                None => region_missing = true,
            },
            "--fullscreen" => a.fullscreen = true,
            _ => {}
        }
    }
    let code = if region_missing {
        // The script has made the save directory by the time it reads its arguments.
        if save_dir(&a.save_path).is_some() {
            super::notify(
                "Recording cancelled",
                "No region specified for --region",
                "Recorder",
            );
        }
        1
    } else {
        run(&a, &Io::detached())
    };
    workpool::wait_idle();
    code
}

pub fn run(a: &Args, io: &Io) -> i32 {
    let Some(dir) = save_dir(&a.save_path) else {
        return 1;
    };
    let pids = recorders();
    if !pids.is_empty() {
        super::notify("Recording Stopped", "Stopped", "Recorder");
        for p in pids {
            // SAFETY: kill(2) takes plain integers and touches no memory of this process.
            unsafe { libc::kill(p, libc::SIGTERM) };
        }
        return 0;
    }
    let audio = if a.silent {
        None
    } else {
        let Some(defaults) = super::songrec::default_names(&io.stop) else {
            return 143;
        };
        audio_arg(&defaults)
    };
    let mut wf = workpool::userenv::command("wf-recorder");
    wf.current_dir(&dir);
    if a.fullscreen {
        super::notify(
            "Starting recording",
            &format!("recording_{}.mp4", super::date_stamp()),
            "Recorder",
        );
        wf.args(["-o", &focused_monitor()]);
        wf.args([
            "--pixel-format",
            "yuv420p",
            "-f",
            &format!("./recording_{}.mp4", super::date_stamp()),
            "-t",
        ]);
        wf.args(&audio);
    } else {
        let region = match &a.region {
            Some(r) if !r.is_empty() => r.clone(),
            _ => match crate::child::run(
                workpool::userenv::command("slurp")
                    .current_dir(&dir)
                    .stdin(Stdio::null()),
                &io.stop,
            ) {
                Ok(o) if o.status.is_some_and(|s| s.success()) => {
                    let mut s = String::from_utf8_lossy(&o.out).into_owned();
                    s.push_str(&String::from_utf8_lossy(&o.err));
                    s.trim_end_matches('\n').to_owned()
                }
                // `running = false` while slurp waits for the region.
                _ if io.stopped() => return 143,
                _ => {
                    super::notify("Recording cancelled", "Selection was cancelled", "Recorder");
                    return 1;
                }
            },
        };
        super::notify(
            "Starting recording",
            &format!("recording_{}.mp4", super::date_stamp()),
            "Recorder",
        );
        wf.args([
            "--pixel-format",
            "yuv420p",
            "-f",
            &format!("./recording_{}.mp4", super::date_stamp()),
            "-t",
        ]);
        wf.args(["--geometry", &region]);
        wf.args(&audio);
    }
    let Ok(ch) = crate::child::spawn(
        wf.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    ) else {
        return 127;
    };
    let watch = super::watch_kill(ch.id() as i32, io.stop.clone());
    let st = ch.wait();
    drop(watch);
    if io.stopped() {
        return 143;
    }
    st.ok().and_then(|s| s.code()).unwrap_or(1)
}

/// `--audio=$(pactl get-default-sink).monitor`: the default sink's monitor. With no default
/// sink there is nothing to capture, and the video is recorded without audio rather than
/// handing wf-recorder a `.monitor` device that does not exist.
pub fn audio_arg(defaults: &[String; 4]) -> Option<String> {
    (!defaults[0].is_empty()).then(|| format!("--audio={}.monitor", defaults[0]))
}
