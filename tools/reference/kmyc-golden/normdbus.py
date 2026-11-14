"""Reduce raw `dbus-monitor --session` output to a deterministic log.

Keeps signals, method calls and errors (header without time/serial/sender, plus the
indented argument lines). Drops method returns, the bus driver's signals and the
connection plumbing calls to it (Hello, AddMatch, RemoveMatch, ...); calls that show
what the program looks for on the bus (ListNames, GetNameOwner, StartServiceByName,
NameHasOwner) stay. Unique connection names (:1.N) become :X.
"""
import re
import sys

HDR = re.compile(r"^(signal|method call|method return|error) ")
UNIQUE = re.compile(r":\d+\.\d+")
DRIVER = "org.freedesktop.DBus"
DRIVER_KEEP = {"ListNames", "GetNameOwner", "StartServiceByName", "NameHasOwner"}


def field(line, name):
    m = re.search(rf"\b{name}=([^ ;]*)", line)
    return m.group(1) if m else None


def sort_dicts(args):
    """dbus-monitor prints a{..} entries in sender order (KConfig: QHash order, random
    per process); sort the entries of every array of dict entries."""
    def parse(i, depth):
        node = []
        while i < len(args):
            line = args[i]
            s = line.strip()
            if s in ("]", ")"):
                return node, i
            if s.endswith(("[", "(")):
                kids, j = parse(i + 1, depth + 1)
                node.append((line, kids, args[j] if j < len(args) else None))
                i = j + 1
            else:
                node.append((line, None, None))
                i += 1
        return node, i

    def emit(node):
        out = []
        for line, kids, close in node:
            out.append(line)
            if kids is not None:
                if kids and all(k[0].strip() == "dict entry(" for k in kids):
                    kids = sorted(kids, key=lambda k: emit([k]))
                out += emit(kids)
                if close is not None:
                    out.append(close)
        return out

    return emit(parse(0, 0)[0])


def main():
    out, args, keep = [], [], False
    for line in sys.stdin.read().splitlines() + ["method return -"]:
        m = HDR.match(line)
        if not m:
            if keep and line.strip():
                args.append(UNIQUE.sub(":X", line.rstrip()))
            continue
        out += sort_dicts(args)
        args = []
        kind = m.group(1)
        sender, dest = field(line, "sender"), field(line, "destination")
        keep = False
        if kind == "method return":
            continue
        if kind == "error":
            keep = True
            out.append(f"error error_name={field(line, 'error_name')}")
            continue
        iface, member, path = field(line, "interface"), field(line, "member"), field(line, "path")
        if sender == DRIVER or (dest == DRIVER and member not in DRIVER_KEEP):
            continue
        parts = [kind.replace(" ", "_")]
        if dest and not dest.startswith((":", "(null")):
            parts.append(f"destination={dest}")
        parts.append(f"path={path}; interface={iface}; member={member}")
        out.append(" ".join(parts))
        keep = True
    sys.stdout.write("".join(l + "\n" for l in out))


main()
