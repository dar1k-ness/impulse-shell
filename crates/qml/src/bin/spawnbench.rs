//! How a `Process` start costs the shell: the old `setpriv --pdeathsig TERM --` wrapper, the
//! native posix_spawn + process group, and a fork-based start (a `pre_exec` hook), from a
//! process holding a large touched heap like the running shell.
//!   spawnbench [HEAP_MB=1024] [N=300]

use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn cpu() -> Duration {
    // SAFETY: `rusage` is a C struct of integers and `timeval`s: all-zero bytes are a valid value.
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `ru` is a valid, writable `rusage` for getrusage(2) to fill.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    let t = |v: libc::timeval| Duration::new(v.tv_sec as u64, v.tv_usec as u32 * 1000);
    t(ru.ru_utime) + t(ru.ru_stime)
}

fn run(name: &str, n: usize, mk: &dyn Fn() -> Command) {
    let (mut spawn, mut total) = (Duration::ZERO, Duration::ZERO);
    let mut worst = Duration::ZERO;
    let c0 = cpu();
    for _ in 0..n {
        let t = Instant::now();
        let mut ch = mk().stdout(Stdio::null()).spawn().expect("spawn");
        let s = t.elapsed();
        ch.wait().unwrap();
        spawn += s;
        worst = worst.max(s);
        total += t.elapsed();
    }
    let us = |d: Duration| d.as_secs_f64() * 1e6 / n as f64;
    println!(
        "{name:<26} spawn() {:>8.1} us (max {:>8.1})  start->exit {:>8.1} us  shell CPU {:>8.1} us/start",
        us(spawn),
        worst.as_secs_f64() * 1e6,
        us(total),
        us(cpu() - c0)
    );
}

fn main() {
    let a: Vec<usize> = std::env::args()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    let (mb, n) = (
        a.first().copied().unwrap_or(1024),
        a.get(1).copied().unwrap_or(300),
    );
    let mut heap = vec![0u8; mb << 20];
    for i in (0..heap.len()).step_by(4096) {
        heap[i] = 1;
    }
    println!("heap {mb} MiB, {n} starts of /bin/true each");
    let sp = ["/usr/bin/setpriv", "/bin/setpriv"]
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file());
    if let Some(sp) = sp {
        run("setpriv --pdeathsig (old)", n, &|| {
            let mut c = Command::new(sp);
            c.args(["--pdeathsig", "TERM", "--", "/bin/true"]);
            c
        });
    }
    run("posix_spawn + pgroup (new)", n, &|| {
        let mut c = Command::new("/bin/true");
        c.arg0("true").process_group(0);
        c
    });
    run("fork + pre_exec pdeathsig", n, &|| {
        let mut c = Command::new("/bin/true");
        let hook = || -> std::io::Result<()> {
            // SAFETY: PR_SET_PDEATHSIG takes a signal number and touches no memory of ours.
            unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) };
            Ok(())
        };
        // SAFETY: the hook only makes the prctl(2) system call between fork and exec: no
        // allocation or lock, as `CommandExt::pre_exec` requires.
        unsafe { c.pre_exec(hook) };
        c
    });
    std::hint::black_box(&heap);
}
