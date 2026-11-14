# Generated documents against Qt 6.12

How the parser is compared with Qt's on documents nobody wrote: a seed with a few of its
tokens removed, doubled, swapped or replaced. Run by hand; it needs the image of
`tools/testenv` for Qt (PySide6 6.12 in `/opt/qv4-venv`).

```sh
python3 tools/gen/qqmljs/fuzz/gen.py /tmp/g1 6000 1          # 6000 documents, random seed 1
docker run --rm -v /tmp/g1:/docs:ro -v $PWD/tools/gen/qqmljs/fuzz:/fuzz:ro IMAGE \
    /opt/qv4-venv/bin/python /fuzz/probe.py /docs > /tmp/g1.qt
(cd /tmp/g1 && cargo run --release -p qml --example parse_status -- G*.qml) > /tmp/g1.ours
python3 tools/gen/qqmljs/fuzz/compare.py /tmp/g1.qt /tmp/g1.ours
```

`probe.py` and `parse_status` print a line for each document: its name, the status of the
component (1 Ready, 3 Error, 0 Null) and its `errorString()`. `compare.py` counts the
documents the two lists agree on and groups the others by Qt's message. The others are
documents Qt rejects after parsing, when it resolves imports and types ("Script lib.js
unavailable", "Cannot assign to non-existent property"): the parser accepts those.

`sample.py` keeps a part of such a run as `crates/qml/tests/qt_parse/generated.tsv`, which
the test `qt_parse` reads: see its first lines.
