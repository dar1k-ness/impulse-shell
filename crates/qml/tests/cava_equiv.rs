//! `qml::cava` against the real `cava` binary: the original's `raw_output_config.txt` with fifo input
//! (48 kHz stereo S16, the format CavaVisualizer gets from PipeWire) and a fifo `raw_target`.
//! `tools/reference/cava-equiv/shim.c` (LD_PRELOAD) steps cava one frame at a time, so both sides get
//! exactly the same blocks before each frame and the output lines must be identical.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use qml::cava::{self, BARS, Input};

const FRAMES: usize = 240;
/// cava's fifo input reads `BUFFER_SIZE * channels` samples at a time.
const BLOCK: usize = 512;

fn pipe() -> (OwnedFd, OwnedFd) {
    let (r, w) = std::io::pipe().unwrap();
    (r.into(), w.into())
}

/// Stereo blocks fed before frame `f` (`None`: make cava's input thread give up waiting, which
/// runs `reset_output_buffers` as when the source goes quiet).
fn blocks(f: usize, lcg: &mut u32) -> Option<Vec<Vec<i16>>> {
    if f == 170 {
        return None;
    }
    let n = match f {
        100 => 20,
        _ => [1, 2, 1, 0, 3, 1, 2][f % 7],
    };
    let loud = (20..120).contains(&f) || f >= 180;
    Some(
        (0..n)
            .map(|b| {
                let amp = 2000 + ((f * 53 + b * 11) % 25000) as i32;
                (0..BLOCK)
                    .flat_map(|t| {
                        *lcg = lcg.wrapping_mul(1103515245).wrapping_add(12345);
                        let noise = ((*lcg >> 16) & 0x7fff) as i32 - 16384;
                        let tone = ((t as i32 * (7 + f as i32 % 13)) % 400 - 200) * 5;
                        let l = if loud { tone * amp / 1000 } else { 0 };
                        let r = if loud { noise * amp / 16384 } else { 0 };
                        [l.clamp(-32768, 32767) as i16, r.clamp(-32768, 32767) as i16]
                    })
                    .collect()
            })
            .collect(),
    )
}

#[test]
fn cava_binary_matches_native() {
    if !testenv::require("cava") || !testenv::require("cc") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let shim = dir.path().join("shim.so");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/reference/cava-equiv/shim.c");
    let st = Command::new("cc")
        .args(["-shared", "-fPIC", "-O2", "-o"])
        .arg(&shim)
        .arg(&src)
        .args(["-ldl"])
        .status()
        .unwrap();
    assert!(st.success(), "building {}", src.display());

    let audio = dir.path().join("audio.fifo");
    let raw = dir.path().join("raw.fifo");
    for p in [&audio, &raw] {
        let c = std::ffi::CString::new(p.to_str().unwrap()).unwrap();
        // SAFETY: `c` is a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    }
    let orig = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/original-unshipped/scripts/cava/raw_output_config.txt"),
    )
    .unwrap();
    let config = orig.replace(
        "raw_target = /dev/stdout",
        &format!("raw_target = {}", raw.display()),
    ) + &format!(
        "\n[input]\nmethod = fifo\nsource = {}\nsample_rate = 48000\nsample_bits = 16\nchannels = 2\n",
        audio.display()
    );
    assert!(config.contains(&raw.display().to_string()));
    let cfg = dir.path().join("config");
    std::fs::write(&cfg, config).unwrap();

    // Opened read-write so neither open blocks and the fifos never see EOF.
    let rw = |p: &Path| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(p)
            .unwrap()
    };
    let mut audio_w = rw(&audio);
    let raw_r = rw(&raw);
    let (frame_r, frame_w) = pipe();
    let (go_r, go_w) = pipe();
    let (data_r, data_w) = pipe();
    let (took_r, took_w) = pipe();
    let (idle_r, idle_w) = pipe();
    let fds = [
        frame_w.as_raw_fd(),
        go_r.as_raw_fd(),
        data_r.as_raw_fd(),
        took_w.as_raw_fd(),
        idle_w.as_raw_fd(),
    ];
    let dup_fds = move || {
        for (i, fd) in fds.iter().enumerate() {
            // SAFETY: dup2(2) takes no pointers; `fd` is a pipe end this test still holds open.
            if unsafe { libc::dup2(*fd, 20 + i as i32) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    };
    let mut cava_cmd = Command::new("cava");
    cava_cmd
        .arg("-p")
        .arg(&cfg)
        .env("LD_PRELOAD", &shim)
        .env("CAVA_SHIM_FRAME", "20")
        .env("CAVA_SHIM_GO", "21")
        .env("CAVA_SHIM_DATA", "22")
        .env("CAVA_SHIM_TOOK", "23")
        .env("CAVA_SHIM_IDLE", "24")
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    // SAFETY: the hook runs between fork and exec; it only calls dup2(2) and reads errno,
    // both async-signal-safe, and allocates nothing.
    unsafe { cava_cmd.pre_exec(dup_fds) };
    let mut child = cava_cmd.spawn().unwrap();
    drop((frame_w, go_r, data_r, took_w, idle_w));
    let (mut frame_r, mut go_w) = (std::fs::File::from(frame_r), std::fs::File::from(go_w));
    let (mut data_w, mut took_r) = (std::fs::File::from(data_w), std::fs::File::from(took_r));
    let mut idle_r = std::fs::File::from(idle_r);
    let mut lines = BufReader::new(raw_r);
    let mut byte = [0u8];
    let mut line = String::new();
    // Frame 0 runs before the first block can arrive.
    lines.read_line(&mut line).unwrap();

    let mut plan = cava::plan();
    let mut input = Input::new(plan.input_buffer_size);
    let mut out = vec![0.0; BARS * 2];
    native_line(&mut plan, &mut input, &mut out, &line).expect("frame 0");

    let mut lcg = 1u32;
    let mut mismatches = vec![];
    let mut moving = 0;
    for f in 0..FRAMES {
        frame_r.read_exact(&mut byte).expect("cava exited");
        match blocks(f, &mut lcg) {
            Some(bs) => {
                for b in bs {
                    let bytes: Vec<u8> = b.iter().flat_map(|s| s.to_le_bytes()).collect();
                    idle_r.read_exact(&mut byte).expect("cava exited");
                    audio_w.write_all(&bytes).unwrap();
                    data_w.write_all(&[0]).unwrap();
                    took_r.read_exact(&mut byte).unwrap();
                    input.write(&b);
                }
            }
            None => {
                for _ in 0..11 {
                    idle_r.read_exact(&mut byte).expect("cava exited");
                    data_w.write_all(&[0]).unwrap();
                }
                took_r.read_exact(&mut byte).unwrap();
                input.reset();
            }
        }
        go_w.write_all(&[0]).unwrap();
        line.clear();
        lines.read_line(&mut line).unwrap();
        moving += usize::from(
            line.split(';')
                .any(|v| !v.trim().is_empty() && v.trim() != "0"),
        );
        if let Err(e) = native_line(&mut plan, &mut input, &mut out, &line) {
            mismatches.push(format!("frame {f}: {e}"));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(moving > FRAMES / 2, "only {moving} frames with bars up");
    assert!(
        mismatches.is_empty(),
        "{} of {FRAMES} frames differ:\n{}",
        mismatches.len(),
        mismatches[..mismatches.len().min(3)].join("\n")
    );
}

fn native_line(
    plan: &mut cava::Plan,
    input: &mut Input,
    out: &mut [f64],
    want: &str,
) -> Result<(), String> {
    let got: String = cava::frame(plan, input, out)
        .iter()
        .map(|v| format!("{v};"))
        .collect();
    let want = want.trim_end_matches('\n');
    (got == want)
        .then_some(())
        .ok_or(format!("\n  native {got}\n  cava   {want}"))
}
