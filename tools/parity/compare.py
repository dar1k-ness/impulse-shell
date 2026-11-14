#!/usr/bin/env python3
"""Compare two capture dirs (orig vs native): diff images, results.json, summary.md, a verdict.

usage: compare.py A_DIR B_DIR OUT_DIR [--masks masks.json] [--policy policy.json]
       compare.py --same A.ppm B.ppm     (capture.sh's idle check: exit 0 when the two screens are equal)

Exit code: 0 every shot passed, 1 at least one shot differs for real, 2 the capture itself is not
valid (tool error), whatever the shots show.

A shot passes when `coarse` is within its limit. `coarse` counts the pixels whose colour still
differs by more than COARSE_DELTA after both images are averaged over BLUR x BLUR pixels: two
rasterisers put the same glyph or curve on slightly different pixels (a few hundred single-pixel
deltas on every shot), which the average cancels, while anything moved, missing or recoloured
stays. Limits are in policy.json, one line per pattern with the measured reason.

Tool errors: a shot missing on one side or of another size; a shot equal to the idle screen of
its own run although its scene opens something (`empty-*`); a scene that did not return to the
idle screen (not-idle.txt from capture.sh).

masks.json: {"*": [[x, y, w, h, "reason"], ...], "scene": [...]} rectangles excluded from the
diff because their content legitimately differs between two runs (uptime, CPU %, ...). A key
`layer:NAME` (or `layer:NAME+NAME`) is not a pattern on the shot's name: its rectangles are left
out of every shot taken while the shell had those layers mapped (layers.txt of the run, written by lib.sh), so
a live value is masked wherever its panel is open, whatever the scene is called.

policy.json `areas`: rectangles of a shot with limits of their own, for a difference the whole-shot
limit cannot see (a button a shade lighter is under COARSE_DELTA). The shot differs when, inside
the rectangle, the mean colour difference or the largest one after the BLUR average is over its
limit.
Animation frames (NAME.fNN.png) are matched to the closest original frame of the same NAME,
since frame timing between two processes cannot be made exact.
"""
import argparse
import fnmatch
import json
import os
import re
import sys

import numpy as np
from PIL import Image

BLUR = 5
COARSE_DELTA = 24
TILE = 16


def load(p):
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def layers(d):
    """{shot: the shell's layers mapped when it was taken} of a capture dir (lib.sh on_screen)."""
    out = {}
    p = os.path.join(d, "layers.txt")
    for line in open(p) if os.path.exists(p) else []:
        name, *on = line.split()
        out.setdefault(name, set()).update(on)
    return out


def mask_for(name, shape, masks, on=()):
    """The pixels compared (True) in shot `name`, taken with the layers `on` mapped."""
    m = np.ones(shape[:2], bool)
    used = []
    for pat, rects in masks.items():
        if pat.startswith("layer:"):
            applies = set(pat[len("layer:"):].split("+")) <= set(on)
        else:
            applies = fnmatch.fnmatch(name, pat)
        if applies:
            for x, y, w, h, why in rects:
                m[y:y + h, x:x + w] = False
                used.append(why)
    return m, used


def regions(diff):
    """Bounding boxes of 8-connected groups of differing TILE x TILE tiles."""
    h, w = diff.shape
    th, tw = -(-h // TILE), -(-w // TILE)
    pad = np.zeros((th * TILE, tw * TILE), bool)
    pad[:h, :w] = diff
    tiles = pad.reshape(th, TILE, tw, TILE).any(axis=(1, 3))
    seen = np.zeros_like(tiles)
    out = []
    for ty, tx in zip(*np.nonzero(tiles)):
        if seen[ty, tx]:
            continue
        stack, box = [(ty, tx)], [ty, tx, ty, tx]
        seen[ty, tx] = True
        while stack:
            y, x = stack.pop()
            box = [min(box[0], y), min(box[1], x), max(box[2], y), max(box[3], x)]
            for dy in (-1, 0, 1):
                for dx in (-1, 0, 1):
                    ny, nx = y + dy, x + dx
                    if 0 <= ny < th and 0 <= nx < tw and tiles[ny, nx] and not seen[ny, nx]:
                        seen[ny, nx] = True
                        stack.append((ny, nx))
        x0, y0 = box[1] * TILE, box[0] * TILE
        x1, y1 = min((box[3] + 1) * TILE, w), min((box[2] + 1) * TILE, h)
        out.append({"x": int(x0), "y": int(y0), "w": int(x1 - x0), "h": int(y1 - y0),
                    "pixels": int(diff[y0:y1, x0:x1].sum())})
    return sorted(out, key=lambda r: -r["pixels"])


def compare(a, b, m):
    if a.shape != b.shape:
        return None
    d = np.abs(a - b).max(axis=2) * m
    return d


def box(img):
    """Mean over BLUR x BLUR windows (edges clamp), per channel."""
    r = BLUR // 2
    p = np.pad(img.astype(np.int32), ((r, r + 1), (r, r + 1), (0, 0)), mode="edge")
    c = p.cumsum(axis=0).cumsum(axis=1)
    h, w = img.shape[:2]
    s = c[BLUR:BLUR + h, BLUR:BLUR + w] - c[:h, BLUR:BLUR + w] - c[BLUR:BLUR + h, :w] + c[:h, :w]
    return s / (BLUR * BLUR)


def coarse(a, b, m):
    """Pixels that still differ after averaging: masked pixels count as equal."""
    return (np.abs(box((a - b) * m[..., None])).max(axis=2) > COARSE_DELTA) & m


def area_check(a, b, m, areas):
    """Each `areas` rectangle [x, y, w, h, mean limit, max limit, reason] measured on a and b."""
    out = []
    if not areas:
        return out
    d = np.abs(a - b) * m[..., None]
    raw, avg = d.max(axis=2), np.abs(box((a - b) * m[..., None])).max(axis=2)
    for x, y, w, h, mean_limit, max_limit, _why in areas:
        mean, top = float(raw[y:y + h, x:x + w].mean()), float(avg[y:y + h, x:x + w].max())
        out.append({"x": x, "y": y, "w": w, "h": h, "mean": round(mean, 2), "max": round(top, 2),
                    "mean_limit": mean_limit, "max_limit": max_limit,
                    "status": "pass" if mean <= mean_limit and top <= max_limit else "differs"})
    return out


def lookup(table, name):
    """The value of the last pattern in `table` that matches `name` (later lines are more specific)."""
    hit = None
    for pat, v in table.items():
        if fnmatch.fnmatch(name, pat):
            hit = v
    return hit


def same_screen(pa, pb, masks, name="*", on=()):
    """Whether two shots of one shell show the same screen: nothing visible differs outside the masks."""
    a, b = load(pa), load(pb)
    if a.shape != b.shape:
        return False, -1
    m, _ = mask_for(name, a.shape, masks, on)
    n = int((compare(a, b, m) > 8).sum())
    return n == 0, n


def write_diff(path, a, b, d, regs):
    """orig | native | orig dimmed with differing pixels in red; region boxes in yellow."""
    vis = (a * 0.35).astype(np.uint8)
    vis[d > 0] = [255, 0, 0]
    for r in regs[:40]:
        x0, y0, x1, y1 = r["x"], r["y"], r["x"] + r["w"] - 1, r["y"] + r["h"] - 1
        vis[y0, x0:x1 + 1] = vis[y1, x0:x1 + 1] = [255, 255, 0]
        vis[y0:y1 + 1, x0] = vis[y0:y1 + 1, x1] = [255, 255, 0]
    strip = np.concatenate([a.astype(np.uint8), b.astype(np.uint8), vis], axis=1)
    Image.fromarray(strip).resize((strip.shape[1] // 2, strip.shape[0] // 2), Image.BILINEAR).save(path)


TOOL_ERRORS = ("missing-a", "missing-b", "size-mismatch", "empty-a", "empty-b", "not-idle")


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser()
    ap.add_argument("orig")
    ap.add_argument("native")
    ap.add_argument("out", nargs="?")
    ap.add_argument("--masks", default=os.path.join(here, "masks.json"))
    ap.add_argument("--policy", default=os.path.join(here, "policy.json"))
    ap.add_argument("--same", action="store_true")
    args = ap.parse_args()
    masks = json.load(open(args.masks)) if os.path.exists(args.masks) else {}
    masks = {k: v for k, v in masks.items() if not k.startswith("_")}
    if args.same:
        ok, n = same_screen(args.orig, args.native, masks, "idle")
        print(f"{n} px")
        sys.exit(0 if ok else 1)
    if not args.out:
        ap.error("OUT_DIR is required")
    policy = json.load(open(args.policy))
    limits, idle_of, idle_shots = policy["limits"], policy["idle_of"], policy["idle_shots"]
    areas = policy.get("areas", {})
    on = {side: layers(d) for side, d in (("a", args.orig), ("b", args.native))}
    os.makedirs(args.out, exist_ok=True)
    sides = {"a": args.orig, "b": args.native}

    frame_re = re.compile(r"^(.*)\.f(\d+)$")

    def shots(d):
        return {os.path.splitext(f)[0] for f in os.listdir(d)
                if f.endswith(".png") and not f.startswith("idle")}

    names = sorted(shots(args.orig) | shots(args.native))
    groups = {}
    for n in names:
        mm = frame_re.match(n)
        groups.setdefault(mm.group(1) if mm else n, []).append(n)

    results = []
    # Scenes that left something open: every later shot of that run has it on screen.
    for side, d in sides.items():
        p = os.path.join(d, "not-idle.txt")
        for line in open(p) if os.path.exists(p) else []:
            scene, _, px = line.strip().partition(" ")
            results.append({"scene": f"scene {scene} ({side})", "status": "not-idle", "note": f"left {px} of the idle screen changed"})
    # What the scenes recorded about actions no shot can show.
    for log in policy.get("logs", []):
        text = [open(p).read().splitlines() if os.path.exists(p := os.path.join(d, log)) else None for d in sides.values()]
        if text[0] is None and text[1] is None:
            continue
        if text[0] is None or text[1] is None:
            results.append({"scene": log, "status": "missing-a" if text[0] is None else "missing-b"})
            continue
        diff = [f"`{x}` / `{y}`" for x, y in zip(text[0], text[1]) if x != y]
        if len(text[0]) != len(text[1]):
            diff.append(f"{len(text[0])} lines / {len(text[1])} lines")
        results.append({"scene": log, "status": "differs" if diff else "pass", "note": "; ".join(diff)})
    # Times the scenes measured ("NAME MS ..." a line): the two runs may be this far apart.
    for log, (limit, why) in policy.get("times", {}).items():
        ms = [dict(x.split()[:2] for x in open(p)) if os.path.exists(p := os.path.join(d, log)) else {}
              for d in sides.values()]
        for name in sorted(set(ms[0]) | set(ms[1])):
            if name not in ms[0] or name not in ms[1]:
                results.append({"scene": f"{log} {name}", "status": "missing-b" if name in ms[0] else "missing-a"})
                continue
            a, b = int(ms[0][name]), int(ms[1][name])
            results.append({"scene": f"{log} {name}", "status": "pass" if abs(a - b) <= limit else "differs",
                            "note": f"{a} ms / {b} ms, may differ by {limit} ms: {why}"})
    unsettled = {side: set(open(p).read().split()) if os.path.exists(p := os.path.join(d, "unsettled.txt")) else set()
                 for side, d in sides.items()}

    for scene, members in groups.items():
        for n in members:
            pa, pb = (os.path.join(d, n + ".png") for d in (args.orig, args.native))
            res = {"scene": n}
            if not os.path.exists(pb) or not os.path.exists(pa):
                res["status"] = "missing-b" if os.path.exists(pa) else "missing-a"
                results.append(res)
                continue
            b = load(pb)
            cands = [n]
            if len(members) > 1 and frame_re.match(n):
                cands = [c for c in members if os.path.exists(os.path.join(args.orig, c + ".png"))]
            best = None
            shown = on["a"].get(n, set()) | on["b"].get(n, set())
            for c in cands:
                a = load(os.path.join(args.orig, c + ".png"))
                m, used = mask_for(n, a.shape, masks, shown)
                d = compare(a, b, m)
                if d is None:
                    continue
                key = int((d > 32).sum())
                if best is None or key < best[0]:
                    best = (key, c, a, d, used, m)
            if best is None:
                res["status"] = "size-mismatch"
                results.append(res)
                continue
            _, c, a, d, used, m = best
            co = coarse(a, b, m)
            ncoarse, cnt = int(co.sum()), int((d > 0).sum())
            limit, why = lookup(limits, n)
            regs = regions(co if ncoarse else d > 8)
            checked = area_check(a, b, m, [r for pat, rects in areas.items() if fnmatch.fnmatch(n, pat) for r in rects])
            res.update({
                "status": "pass" if ncoarse <= limit and all(r["status"] == "pass" for r in checked) else "differs",
                "areas": checked,
                "matched_orig": c,
                "coarse": ncoarse,
                "limit": limit,
                "limit_reason": why,
                "pixels": cnt,
                "pixels_gt8": int((d > 8).sum()),
                "pixels_gt32": int((d > 32).sum()),
                "max": int(d.max()),
                "masked": sorted(set(used)),
                "regions": regs[:20],
                "unsettled": sorted(s for s in sides if n in unsettled[s]),
            })
            # A shot whose scene opens something must not show the idle screen of its own run.
            if not frame_re.match(n) and lookup(idle_shots, n) is None:
                for side, sd in sides.items():
                    ref = os.path.join(sd, (lookup(idle_of, n) or "idle") + ".png")
                    if os.path.exists(ref) and same_screen(ref, os.path.join(sd, n + ".png"), masks, n, on[side].get(n, ()))[0]:
                        res["status"] = f"empty-{side}"
                        res["note"] = f"equal to {os.path.basename(ref)} of its run: the scene shot nothing"
                        break
            if cnt:
                write_diff(os.path.join(args.out, n + ".diff.png"), a, b, d, regs)
            results.append(res)

    json.dump(results, open(os.path.join(args.out, "results.json"), "w"), indent=1)
    count = {k: sum(r["status"] == k for r in results) for k in ("pass", "differs")}
    errors = [r for r in results if r["status"] in TOOL_ERRORS]
    verdict = 2 if errors else 1 if count["differs"] else 0
    with open(os.path.join(args.out, "summary.md"), "w") as f:
        f.write(f"{len(results)} rows: {count['pass']} pass, {count['differs']} differ, {len(errors)} tool errors; "
                f"exit code {verdict}\n\n"
                f"`coarse`: pixels whose colour differs by more than {COARSE_DELTA} of 255 after both shots are "
                f"averaged over {BLUR}x{BLUR} pixels, outside the masks. A shot passes when `coarse` <= `limit` "
                f"and every area of it is within its own limits.\n\n")
        for pat, (lim, why) in limits.items():
            f.write(f"- limit {lim} for `{pat}`: {why}\n")
        for pat, rects in areas.items():
            for x, y, w, h, mean_limit, max_limit, why in rects:
                f.write(f"- area {x},{y},{w},{h} of `{pat}`: mean difference <= {mean_limit}, largest after the "
                        f"{BLUR}x{BLUR} average <= {max_limit}: {why}\n")
        for side, d in sides.items():
            if os.path.exists(p := os.path.join(d, "build.txt")):
                f.write(f"- native build ({side}): `{open(p).read().strip()}`\n")
        f.write("\n")
        f.write("| scene | status | coarse | limit | px differ | >8 | >32 | max | regions (x,y,w,h) | note |\n"
                "|---|---|---|---|---|---|---|---|---|---|\n")
        for r in results:
            regs = "; ".join(f"{g['x']},{g['y']},{g['w']},{g['h']}" for g in r.get("regions", [])[:4])
            note = r.get("note", "")
            for g in r.get("areas", []):
                note = (note + " " if note else "") + (f"area {g['x']},{g['y']},{g['w']},{g['h']}: mean {g['mean']} "
                                                       f"(limit {g['mean_limit']}), max {g['max']} (limit {g['max_limit']})")
            if r.get("unsettled"):
                note = (note + " " if note else "") + "screen still changing when shot: " + ",".join(r["unsettled"])
            f.write(f"| {r['scene']} | {r['status']} | {r.get('coarse', '')} | {r.get('limit', '')} "
                    f"| {r.get('pixels', '')} | {r.get('pixels_gt8', '')} "
                    f"| {r.get('pixels_gt32', '')} | {r.get('max', '')} | {regs} | {note} |\n")
    for r in results:
        if r["status"] != "pass":
            print(f"{r['status']:>13}  {r['scene']}  {r.get('coarse', '')} {r.get('note', '')}", file=sys.stderr)
    print(f"{len(results)} rows: {count['pass']} pass, {count['differs']} differ, {len(errors)} tool errors; "
          f"details in {args.out}/summary.md", file=sys.stderr)
    sys.exit(verdict)


if __name__ == "__main__":
    main()
