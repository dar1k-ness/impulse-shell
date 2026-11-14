#!/usr/bin/env python3
"""Share identical fixture files between cases to keep tests/kde/expected small.

`dedup DIR`: for every file whose bytes equal the file at the same relative path in an
earlier case (seed-*, sv*, defaults-*, then the rest, each by name), the copy is removed and `<case>/shared.txt` gets a line
`<relpath>\t<owner case>`; the owner case always holds the real file.
`expand DIR`: inverse (restores the copies and removes shared.txt).
"""
import os
import shutil
import sys

SHARED = "shared.txt"
FIRST = ("seed-", "sv", "defaults-")


def order(c):
    return (next((i for i, p in enumerate(FIRST) if c.startswith(p)), len(FIRST)), c)


def case_files(d):
    out = []
    for dp, _, fs in os.walk(d):
        out += [os.path.relpath(os.path.join(dp, f), d) for f in fs]
    return sorted(f for f in out if f != SHARED)


def expand(root):
    for c in sorted(os.listdir(root)):
        lst = os.path.join(root, c, SHARED)
        if not os.path.exists(lst):
            continue
        with open(lst, encoding="utf-8") as f:
            for line in f.read().splitlines():
                rel, owner = line.split("\t")
                dst = os.path.join(root, c, rel)
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                shutil.copyfile(os.path.join(root, owner, rel), dst)
        os.remove(lst)


def dedup(root):
    seen = {}
    for c in sorted(os.listdir(root), key=order):
        shared = []
        for rel in case_files(os.path.join(root, c)):
            p = os.path.join(root, c, rel)
            with open(p, "rb") as f:
                key = (rel, f.read())
            if key in seen:
                shared.append(f"{rel}\t{seen[key]}\n")
                os.remove(p)
            else:
                seen[key] = c
        for dp, _, _ in sorted(os.walk(os.path.join(root, c)), reverse=True):
            if dp != os.path.join(root, c) and not os.listdir(dp):
                os.rmdir(dp)
        if shared:
            with open(os.path.join(root, c, SHARED), "w", encoding="utf-8") as f:
                f.writelines(shared)


{"dedup": dedup, "expand": expand}[sys.argv[1]](sys.argv[2])
