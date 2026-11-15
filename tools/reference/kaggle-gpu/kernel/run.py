"""impulse-shell on Kaggle's NVIDIA GPU against llvmpipe, without a display.

Runs the ri-gpushot bundle (tools/reference/kaggle-gpu/bundle.sh, attached as a dataset): each scene is
painted once per frame and the same draw list is rendered on the NVIDIA EGL device and on
Mesa's llvmpipe, so the PNGs differ only by driver. Everything lands in /kaggle/working/out
(frames, diff images, report.json per scene, summary.json) and out.zip. With several bundles
in the input (ri-gpushot-*.tar.xz, e.g. a base and a branch build) every one runs, each into its
own folder, and GPUSHOT_BENCH=N (default 100) re-renders the steady frame N times per backend
printing the time and GL calls per frame (`gpushot --bench`; older bundles lack it).
`run.py --discover` stops after finding the bundles and writing the EGL vendor dir (the part
that runs before any GPU work; tools/reference/kaggle-gpu/check-kernel.sh exercises it on a fake input).
"""
import glob, json, os, shutil, subprocess, sys, tarfile, zipfile

INPUT = os.environ.get("KAGGLE_INPUT", "/kaggle/input")
WORK = os.environ.get("KAGGLE_WORKING", "/kaggle/working")
OUT = os.path.join(WORK, "out")
TMP = os.environ.get("KAGGLE_TMP", "/kaggle/tmp" if os.path.isdir("/kaggle") else "/tmp")
SCENES = [
    ("bar-dark-to-light", "bar", "dark.json", "light.json"),
    ("bar-light-to-dark", "bar", "light.json", "dark.json"),
    ("settings-dark-to-light", "settings", "dark.json", "light.json"),
]
FRAMES = os.environ.get("GPUSHOT_FRAMES", "40")
# Steady frames differ by <= 2 levels (T4 vs llvmpipe). A fade frame composites two 8-bit
# images with a rounding per step, so drivers drift by 1 level almost everywhere and by 3 at
# ~0.03% of pixels (glyph edges; T4 kernel v1). A broken fade, like the RX 580 column shift,
# is off by far more than that.
TOL = os.environ.get("GPUSHOT_TOL", "3")
BENCH = os.environ.get("GPUSHOT_BENCH", "100")


def sh(cmd, **kw):
    print("+", cmd if isinstance(cmd, str) else " ".join(cmd), flush=True)
    return subprocess.run(cmd, shell=isinstance(cmd, str), **kw)


def find_bundles():
    """(label, dir) of every bundle in the input, unpacked under TMP."""
    bundles = locate_bundles()
    for _, b in bundles:
        # Kaggle's input mounts (and zips) may drop the exec bits.
        for f in ["run-gpushot", "bin/gpushot", "lib/ld-linux-x86-64.so.2"]:
            os.chmod(os.path.join(b, f), 0o755)
    return bundles


def locate_bundles():
    found = []
    for run in glob.glob(os.path.join(INPUT, "**/run-gpushot"), recursive=True):
        d = os.path.dirname(run)
        dst = os.path.join(TMP, "unpacked%d" % len(found), "ri-gpushot")
        shutil.rmtree(dst, ignore_errors=True)
        shutil.copytree(d, dst, symlinks=True)
        found.append((os.path.basename(os.path.dirname(d)) or "bundle", dst))
    if found:
        return found
    for arc in sorted(glob.glob(os.path.join(INPUT, "**/ri-gpushot*.tar*"), recursive=True)
                      + glob.glob(os.path.join(INPUT, "**/ri-gpushot*.zip"), recursive=True)):
        tmp = os.path.join(TMP, "unpacked%d" % len(found))
        os.makedirs(tmp, exist_ok=True)
        if arc.endswith(".zip"):
            zipfile.ZipFile(arc).extractall(tmp)
        else:
            tarfile.open(arc).extractall(
                tmp, **({"filter": "tar"} if hasattr(tarfile, "tar_filter") else {}))
        hits = glob.glob(os.path.join(tmp, "**/run-gpushot"), recursive=True)
        if hits:
            label = os.path.basename(arc).split(".")[0].replace("ri-gpushot", "").strip("-")
            found.append((label or "bundle", os.path.dirname(hits[0])))
    if not found:
        sys.exit(f"no ri-gpushot bundle under {INPUT}")
    return found


def vendor_dir(bundle):
    """EGL vendor ICDs: the NVIDIA one plus Mesa's from the bundle (llvmpipe is the reference)."""
    vend = os.path.join(TMP, "egl_vendor.d")
    os.makedirs(vend, exist_ok=True)
    json.dump({"file_format_version": "1.0.0", "ICD": {"library_path": "libEGL_nvidia.so.0"}},
              open(os.path.join(vend, "10_nvidia.json"), "w"))
    shutil.copy(os.path.join(bundle, "egl_vendor.d", "50_mesa.json"), vend)
    return vend


def nvidia_lib_dir():
    """Directory with libEGL_nvidia.so.0, unpacking the matching driver if the image lacks it."""
    for pat in [
        "/usr/lib/x86_64-linux-gnu/libEGL_nvidia.so.0",
        "/usr/lib64/libEGL_nvidia.so.0",
        "/usr/local/nvidia/lib64/libEGL_nvidia.so.0",
        "/usr/local/nvidia/lib/libEGL_nvidia.so.0",
        "/usr/lib/x86_64-linux-gnu/nvidia/current/libEGL_nvidia.so.0",
    ]:
        if os.path.exists(pat):
            return os.path.dirname(pat)
    ver = subprocess.run(
        ["nvidia-smi", "--query-gpu=driver_version", "--format=csv,noheader"],
        capture_output=True, text=True,
    ).stdout.split()[0]
    print(f"no libEGL_nvidia in the image; unpacking driver {ver}", flush=True)
    run = os.path.join(TMP, f"NVIDIA-{ver}.run")
    url = f"https://us.download.nvidia.com/tesla/{ver}/NVIDIA-Linux-x86_64-{ver}.run"
    if sh(["curl", "-fsSL", "-o", run, url]).returncode:
        url = f"https://us.download.nvidia.com/XFree86/Linux-x86_64/{ver}/NVIDIA-Linux-x86_64-{ver}.run"
        sh(["curl", "-fsSL", "-o", run, url], check=True)
    ext = os.path.join(TMP, f"nvidia-{ver}")
    shutil.rmtree(ext, ignore_errors=True)
    sh(["sh", run, "-x", "--target", ext], check=True, stdout=subprocess.DEVNULL)
    lib = os.path.join(TMP, "nvidia-lib")
    os.makedirs(lib, exist_ok=True)
    # Only the user-space EGL/GLES pieces: the kernel module and libcuda stay the host's.
    for f in glob.glob(os.path.join(ext, "*.so." + ver)):
        n = os.path.basename(f)
        if n.startswith(("libEGL_nvidia", "libGLESv2_nvidia", "libnvidia-")) and "cuda" not in n:
            shutil.copy(f, lib)
            soname = n[: -len(ver)] + ("0" if n.startswith(("libEGL_", "libGLESv2_")) else ver)
            if n.startswith("libGLESv2_nvidia"):
                soname = "libGLESv2_nvidia.so.2"
            if soname != n:
                os.symlink(n, os.path.join(lib, soname))
    return lib


def main():
    bundles = find_bundles()
    vend = vendor_dir(bundles[0][1])
    for label, b in bundles:
        print("bundle", label, b, open(os.path.join(b, "REVISION")).read().strip()
              if os.path.exists(os.path.join(b, "REVISION")) else "")
    if "--discover" in sys.argv[1:]:
        return
    sh("nvidia-smi")
    nv = nvidia_lib_dir()
    env = dict(os.environ, EXTRA_LIB_PATH=nv, EGL_VENDOR_DIRS=vend, XDG_CACHE_HOME=TMP)
    shutil.rmtree(OUT, ignore_errors=True)
    summary = []
    for label, b in bundles:
        run_bundle(label, b, env, summary, len(bundles) > 1)
    json.dump(summary, open(os.path.join(OUT, "summary.json"), "w"), indent=1)
    shutil.make_archive(os.path.join(WORK, "out"), "zip", OUT)
    print("\n== summary ==")
    for e in summary:
        rep = e.get("report", {})
        frames = rep.get("frames", [])
        worst = max((f["over_tol"] for f in frames), default=None)
        print(f"{e['name']:28} exit={e['exit']} backends={rep.get('backends')} "
              f"frames_over_tol={sum(1 for f in frames if f['over_tol'])} worst_px={worst}")
    for e in summary:
        for line in e.get("bench", []):
            print(e["name"], line)


def run_bundle(label, b, env, summary, prefix):
    for name, scene, a, z in SCENES:
        if prefix:
            name = f"{label}/{name}"
        out = os.path.join(OUT, name)
        # The NVIDIA device first, llvmpipe last: llvmpipe is the reference.
        r = sh([os.path.join(b, "run-gpushot"), os.path.join(b, "tree"), out, "--scene", scene,
                "--egl", "device", "--egl", "software", "--theme", os.path.join(b, a),
                os.path.join(b, z), "--frames", FRAMES, "--tol", TOL], env=env)
        rep = os.path.join(out, "report.json")
        entry = {"name": name, "exit": r.returncode}
        if os.path.exists(rep):
            entry["report"] = json.load(open(rep))
        summary.append(entry)
    if BENCH == "0":
        return
    for scene in ["bar", "settings"]:
        name = f"{label}/bench-{scene}" if prefix else f"bench-{scene}"
        out = os.path.join(OUT, name)
        r = sh([os.path.join(b, "run-gpushot"), os.path.join(b, "tree"), out, "--scene", scene,
                "--egl", "device", "--egl", "software", "--bench", BENCH], env=env,
               stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        print(r.stdout)
        lines = [l for l in r.stdout.splitlines() if l.startswith("bench ")]
        if r.returncode == 2 and "unknown argument --bench" in r.stdout:
            lines = ["bench: this bundle's gpushot has no --bench"]
        summary.append({"name": name, "exit": r.returncode, "bench": lines})


if __name__ == "__main__":
    main()
