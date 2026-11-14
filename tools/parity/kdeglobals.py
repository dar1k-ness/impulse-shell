#!/usr/bin/env python3
"""kdeglobals.py FILE: pin the keys QKdeTheme reads: fonts and icons to the end-4 dots values, no cursor blink.

Edits FILE in place (keeps every other group and key, e.g. the ColorScheme switchwall.sh writes).
"""
import sys

PINNED = {
    "General": {
        "font": "Google Sans Flex,11,-1,5,500,0,0,0,0,0,0,0,0,0,0,1,Medium",
        "fixed": "JetBrainsMono Nerd Font,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1",
        "menuFont": "Google Sans Flex,10,-1,5,500,0,0,0,0,0,0,0,0,0,0,1,Medium",
        "smallestReadableFont": "Google Sans Flex,9,-1,5,500,0,0,0,0,0,0,0,0,0,0,1,Medium",
        "toolBarFont": "Google Sans Flex,10,-1,5,500,0,0,0,0,0,0,0,0,0,0,1,Medium",
    },
    "Icons": {"Theme": "breeze-dark"},
    # 0: the text cursor does not blink, so a shot of a text field does not depend on the moment.
    "KDE": {"CursorBlinkRate": "0"},
}


def pin(lines):
    out, group, todo = [], None, {g: dict(k) for g, k in PINNED.items()}

    def flush():
        for k, v in todo.get(group, {}).items():
            out.append(f"{k}={v}\n")
        todo.pop(group, None)

    for line in lines:
        s = line.strip()
        if s.startswith("["):
            if out and out[-1].strip() == "":
                blank = out.pop()
                flush()
                out.append(blank)
            else:
                flush()
            group = s[1 : s.index("]")]
        elif "=" in s and group in PINNED:
            k = s.split("=", 1)[0].strip()
            if k in PINNED[group]:
                line = f"{k}={PINNED[group][k]}\n"
                todo.get(group, {}).pop(k, None)
        out.append(line)
    flush()
    for g, keys in todo.items():
        out.append(f"\n[{g}]\n" + "".join(f"{k}={v}\n" for k, v in keys.items()))
    return out


if __name__ == "__main__":
    path = sys.argv[1]
    try:
        with open(path) as f:
            lines = f.readlines()
    except FileNotFoundError:
        lines = []
    new = pin(lines)
    if new != lines:
        with open(path, "w") as f:
            f.writelines(new)
