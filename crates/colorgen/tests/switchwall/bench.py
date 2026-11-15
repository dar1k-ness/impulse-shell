#!/usr/bin/env python3
"""Original switchwall.sh vs the Rust port on the gen.sh sandboxes: processes started, wall
time until the last descendant exits, and CPU (user+sys) of the whole process tree.
Usage: KEEP_WORK=W VENV=V gen.sh; bench.py W <swbench binary> [runs]"""
import ctypes, os, resource, shutil, statistics, subprocess, sys, time

work, swbench = sys.argv[1], sys.argv[2]
runs = int(sys.argv[3]) if len(sys.argv) > 3 else 5
ctypes.CDLL(None).prctl(36, 1)  # PR_SET_CHILD_SUBREAPER: orphaned `&` jobs stay ours


def measure(argv, env, cwd):
    r0 = resource.getrusage(resource.RUSAGE_CHILDREN)
    t0 = time.monotonic()
    p = subprocess.Popen(argv, env=env, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    while True:
        try:
            os.wait()
        except ChildProcessError:
            break
    t = time.monotonic() - t0
    r1 = resource.getrusage(resource.RUSAGE_CHILDREN)
    return t, (r1.ru_utime - r0.ru_utime) + (r1.ru_stime - r0.ru_stime)


def execs(argv, env, cwd):
    log = os.path.join(cwd, "strace.log")
    subprocess.run(["strace", "-f", "-qq", "-e", "trace=execve", "-e", "signal=none", "-o", log] + argv,
                   env=env, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    n = sum(1 for l in open(log) if "execve(" in l and "= 0" in l)
    os.remove(log)
    return n


for case in ["img_dark_auto", "noswitch_color"]:
    pristine = os.path.join(work, case)
    args = open(pristine + ".args").read().split("\n")[:-1]
    res = {}
    for kind in ["legacy", "rust"]:
        ts, cs = [], []
        for i in range(runs + 1):
            h = pristine + ".run"
            shutil.rmtree(h, ignore_errors=True)
            shutil.copytree(pristine, h, symlinks=True)
            a = [x.replace("@W@", f"{h}/Pictures/Wallpapers") for x in args]
            env = {"HOME": h, "PATH": f"{work}/bin:{os.environ['HOME']}/.cargo/bin:/usr/bin:/bin", "SW_LOG": f"{h}/calls.log",
                   "SW_PTS": f"{h}/pts", "SW_FIX": os.environ.get("SW_FIX", ""), "VENV": os.environ["VENV"],
                   "IMPULSE_SHELL_VIRTUAL_ENV": os.environ["VENV"]}
            argv = (["bash", f"{h}/.config/rustshell/impulse-shell/scripts/colors/switchwall.sh"] + a) if kind == "legacy" else [swbench, h] + a
            if i == 0:
                n = execs(argv, env, h)
                continue
            t, c = measure(argv, env, h)
            ts.append(t); cs.append(c)
        res[kind] = (n, statistics.median(ts), statistics.median(cs))
        print(f"{case:15} {kind:6} execve={n:4} wall={res[kind][1]*1000:7.0f} ms cpu={res[kind][2]*1000:7.0f} ms (median of {runs})", flush=True)
