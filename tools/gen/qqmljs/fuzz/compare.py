"""compare.py QT OURS: the documents the two lists disagree on."""
import json, re, sys
PARSER = re.compile(r"^[^:]+:\d+:\d+: ")
def read(p):
    d = {}
    for l in open(p, encoding="utf-8", errors="surrogateescape"):
        name, status, text = l.rstrip("\n").split(" ", 2)
        d[name] = (int(status), json.loads(text))
    return d
qt, ours = read(sys.argv[1]), read(sys.argv[2])
LATER = re.compile(r"is not a type|Cannot assign|non-existent|unavailable|not installed|Invalid property assignment|Invalid alias|Invalid attached|Cannot override|Duplicate|Non-existent|is not available|Type .* unavailable|No such file|Component objects cannot|Invalid grouped|Attached properties cannot|Invalid property type|is ambiguous|cannot be nested|nested|Could not|recursive|read-only|Unable to assign|Property .* already|must be|Singleton|not supported|Illegal method name|Element is not creatable|No property alias|Cannot|cannot|import|Script |module |Expected parameter type|Invalid signal parameter|Unknown pragma|Empty pragma|Multiple .* pragmas|in pragma|depth|Composite Singleton|Required prop|Redundant|declaration inside grouped|set multiple times|Unexpected object definition|not a valid|alias|Invalid empty URL|Invalid use of|Using|value|circular")
same = accept = later = 0
bad = []
for name, (qs, qe) in sorted(qt.items()):
    os_, oe = ours.get(name, (None, None))
    if os_ != 1:
        if qe == oe and qs == os_:
            same += 1
        else:
            bad.append((name, qe, oe))
    else:
        if qs == 1:
            accept += 1
        else:
            bad.append((name, qe, oe))
print(f"{len(qt)} documents: {same} rejected with the same text, {accept} accepted by both, {len(bad)} differ")
kinds = {}
for name, qe, oe in bad:
    k = re.sub(r"^[^:]+:\d+:\d+: ", "", qe.split("\n")[0])
    k = re.sub(r"[`'\"].*", "", k)
    kinds.setdefault(k, []).append(name)
for k, v in sorted(kinds.items(), key=lambda x: -len(x[1])):
    print(f"  {len(v):5}  {k!r}  e.g. {v[:4]}")
if len(sys.argv) > 3:
    for name, qe, oe in bad[: int(sys.argv[3])]:
        print(name, "\n  qt:  ", json.dumps(qe), "\n  ours:", json.dumps(oe))
