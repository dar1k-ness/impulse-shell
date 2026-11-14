"""sample.py OUT [DIR QT OURS]...: the sample of generated documents the test
crates/qml/tests/qt_parse.rs runs, with what Qt 6.12 says about each.

DIR holds the documents of gen.py, QT the answers of probe.py for them and OURS those of
`cargo run -p qml --example parse_status`. Up to three documents for each list of messages
Qt gives (places aside), three hundred it accepts and those it gives up on in silence.
Only documents the two agree on: Qt goes on to resolve imports and types, and what it
rejects there is not the parser's to reject.

A line of OUT: status, seed, errorString (the document is G.qml), then the edits that turn
the seed into the document, each `start,end,text` in bytes of the seed. Tabs between the
fields; `\\\\`, `\\n`, `\\r` and `\\t` in a field are a backslash, a newline, a return and a tab.
"""
import difflib, json, os, re, sys

here = os.path.dirname(os.path.abspath(__file__))
seeds = {f: open(os.path.join(here, "seeds", f)).read() for f in sorted(os.listdir(os.path.join(here, "seeds")))}
PLACE = re.compile(r"^[^:]+:\d+(:\d+)?: ")
TOKEN = re.compile(r"\s+|\w+|.", re.S)


def read(p):
    d = {}
    for l in open(p, encoding="utf-8"):
        name, status, text = l.rstrip("\n").split(" ", 2)
        d[name] = (int(status), json.loads(text))
    return d


def esc(s):
    return s.replace("\\", "\\\\").replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")


def edits(doc):
    # The seed is the one the document shares the most of its ends with; the tokens between
    # those ends are compared.
    def ends(seed):
        a = next((i for i, (x, y) in enumerate(zip(seed, doc)) if x != y), min(len(seed), len(doc)))
        rest = min(len(seed), len(doc)) - a
        b = next((i for i in range(rest) if seed[-1 - i] != doc[-1 - i]), rest)
        return a, b

    name = max(seeds, key=lambda n: sum(ends(seeds[n])))
    seed = seeds[name]
    a, b = ends(seed)
    old, new = TOKEN.findall(seed[a : len(seed) - b]), TOKEN.findall(doc[a : len(doc) - b])
    at = lambda toks, i: sum(len(t) for t in toks[:i])
    out = []
    for op, i1, i2, j1, j2 in difflib.SequenceMatcher(None, old, new, autojunk=False).get_opcodes():
        if op != "equal":
            start, end = a + at(old, i1), a + at(old, i2)
            text = doc[a + at(new, j1) : a + at(new, j2)]
            out.append(f"{len(seed[:start].encode())},{len(seed[:end].encode())},{esc(text)}")
    return name, out


taken, lines = {}, []
args = sys.argv[2:]
for d, qt, ours in zip(args[0::3], args[1::3], args[2::3]):
    qt, ours = read(qt), read(ours)
    for name, (status, text) in sorted(qt.items()):
        if ours.get(name) != (status, text):
            continue
        kind = (status, tuple(PLACE.sub("", l) for l in text.split("\n")))
        taken[kind] = taken.get(kind, 0) + 1
        if taken[kind] > (300 if status == 1 else 3):
            continue
        doc = open(os.path.join(d, name + ".qml")).read()
        seed, ed = edits(doc)
        lines.append("\t".join([str(status), seed, esc(text.replace(name + ".qml", "G.qml"))] + ed))
open(sys.argv[1], "w").write("\n".join(lines) + "\n")
print(len(lines), "documents,", len(taken), "kinds")
