"""Pass 3 of rename.py: the old short name `ii` where it is a word of its own.

In prose (comments, documents, messages) `ii` was the original project, so it becomes "the
original"; where it named this shell's config or tree it becomes what those are called now.
In Rust code `ii` was a name for this shell's own tree, so it becomes `ri`. Values a file of
the user may still hold (`"ii"` in quotes) are not words of prose and stay.
"""

import re

# identifiers that are not the project's name
NOT_OURS = ("crates/qsg/src/shape.rs", "crates/qsg/src/pathsimplify.rs")

# (`-c ii` is the config's name in a command, which the document quotes as it was run)
BARE = r"(?<![\w/.\-\"'`$=\\{<])(?<!-c )ii(?![\w/\-\"'`=(\\}>]|\.\w)"


def _cap(m, word, first):
    before = m.string[: m.start()]
    if (first and not before.strip(" \t/!*#>-")) or re.search(r"[.!?]\s+$", before):
        return word[0].upper() + word[1:]
    return word


# What a file of the user's or a page of the original's says: not this project's words.
KEPT = re.compile(r"\(ii wiki\)|ii shell overrides|Mozilla/5\.0")


def prose(s, first=False):
    """`s` is prose; `first` says a sentence may start where it starts."""
    if KEPT.search(s):
        return s
    sub = lambda rx, word, s: re.sub(rx, lambda m: _cap(m, word, first), s)
    s = re.sub(r"\bnot ii's tree\b", "not the compiled QML tree", s)
    s = re.sub(r"\bnamed ii\b(?![-/])", "named rusty-impulse", s)
    s = re.sub(r"\b(real|whole|stock) ii\b(?![-/'])", r"\1 original", s)
    s = re.sub(r"\b(compiled) ii\b(?![-/'])", r"\1 QML", s)
    s = re.sub(r"\b(\w+'s|own|one) ii (?=[a-z])", r"\1 ", s)
    s = re.sub(BARE[:-1] + r"|\.\w)(?= (assets|scripts|code|runtime|sources?|modules|startup)\b)", "QML", s)
    s = re.sub(r"\b(the original|original) ii's", r"\1's", s)
    s = re.sub(r"\b(the original|original) ii\b(?![-/])", r"\1", s)
    s = re.sub(r"`ii` config", "`rusty-impulse` config", s)
    s = re.sub(r"\b([Aa])n ii config", r"\1 rusty-impulse config", s)
    s = re.sub(BARE[:-1] + r"|\.\w)(?= config)", "rusty-impulse", s)
    s = re.sub(r"\b([Aa])n ii tree", r"\1 QML tree", s)
    s = re.sub(BARE[:-1] + r"|\.\w)(?= tree)", "QML", s)
    s = sub(r"(?<![\w/.\-\"'`$=\\{<])ii's(?!\w)", "the original's", s)
    s = re.sub(r"\b(the|The|this|This|its|Its) " + BARE, r"\1 original", s)
    s = re.sub(r"\b([Aa])n? " + BARE, r"\1n original", s)
    s = re.sub(r"\b([Ee]very|[Ee]ach|[Aa]ny|[Aa]nother|[Nn]o) " + BARE + r"(?= [a-zA-Z])", r"\1 original", s)
    return sub(BARE, "the original", s)


def name(m):
    """A name in snake case with `ii` as one of its words."""
    t = m.group(0)
    if re.search(r"ii_[A-Z]", t) or "ii" not in t.split("_") + [p + "s" for p in ()]:
        if not re.search(r"(^|_)iis(_|$)", t):
            return t
    t = re.sub(r"(^|_)an_ii_rs(?=_|$)", r"\1a_rusty_impulse", t)
    t = re.sub(r"(^|_)ii_rs(?=_|$)", r"\1rusty_impulse", t)
    t = re.sub(r"(^|_)(ii_s|iis)(?=_|$)", r"\1the_original_s", t)
    t = re.sub(r"(^|_)an_ii_config(?=_|$)", r"\1a_rusty_impulse_config", t)
    t = re.sub(r"(^|_)ii_config(?=_|$)", r"\1rusty_impulse_config", t)
    if len([p for p in t.split("_") if p]) >= 4:
        t = re.sub(r"(^|_)(the|an)_ii(?=_|$)", r"\1\2_original", t)
        t = re.sub(r"(^|_)ii(?=_|$)", r"\1the_original", t)
    else:
        t = re.sub(r"(^|_)ii(?=_|$)", r"\1ri", t)
    return t


NAME = re.compile(r"(?<![\w-])[a-z0-9]+(?:_+[a-z0-9]+)+(?![\w-])|(?<![\w-])_+ii(?![\w-])")


def names(s):
    return NAME.sub(name, s)


def rust(text, rel):
    """The text of a Rust file with its comments, strings and code each put right."""
    out, i, n = [], 0, len(text)
    tail = None  # how the comment on the line before ended, None when it was code
    start = 0

    def code(j):
        nonlocal tail
        s = text[start:j]
        if s.strip():
            tail = None
        if rel not in NOT_OURS:
            s = re.sub(r"(?<![\w\"'])ii(?![\w\"'])", "ri", names(s))
        out.append(s)

    while i < n:
        c = text[i]
        if text.startswith("//", i):
            code(i)
            j = text.find("\n", i)
            j = n if j < 0 else j
            body = text[i:j]
            first = tail is None or tail in ".:!?"
            out.append(names(prose(body, first)))
            stripped = body.rstrip()
            tail = stripped[-1] if stripped.strip("/! ") else "."
            i = start = j
        elif text.startswith("/*", i):
            code(i)
            j = text.find("*/", i)
            j = n if j < 0 else j + 2
            out.append(names(prose(text[i:j])))
            i = start = j
        elif c == '"' or (c == "r" and re.match(r'r#*"', text[i:]) and not text[i - 1 : i].isalnum()):
            code(i)
            if c == "r":
                hashes = re.match(r"r(#*)", text[i:]).group(1)
                j = text.find('"' + hashes, i + 2 + len(hashes))
                j = n if j < 0 else j + 1 + len(hashes)
            else:
                j = i + 1
                while j < n and text[j] != '"':
                    j += 2 if text[j] == "\\" else 1
                j = min(n, j + 1)
            lit = text[i:j]
            a = lit.index('"') + 1
            b = max(a, lit.rindex('"')) if lit.endswith(('"', "#")) and lit.rindex('"') >= a else len(lit)
            inner = lit[a:b]
            if inner != "ii" and '= \\"ii\\"' not in inner and '= "ii"' not in inner:
                inner = re.sub(r"\{ii(?=[}:])", "{ri", names(prose(inner)))
            out.append(lit[:a] + inner + lit[b:])
            i = start = j
        elif c == "'":
            m = re.match(r"'(\\.[^']*|[^'\\])'", text[i:])
            i += len(m.group(0)) if m else 1
        else:
            i += 1
    code(n)
    return "".join(out)


def marked(text, marker):
    """A script or a QML file: what follows `marker` on a line is prose, the rest is code."""
    lines = text.split("\n")
    tail = None
    for k, line in enumerate(lines):
        m = re.search(r"(?:^|(?<=\s))" + marker, line) if marker == "#" else re.search(r"(?<![:\w\"'])//", line)
        if not m or (k == 0 and line.startswith("#!")):
            if line.strip():
                tail = None
            continue
        first = tail is None or tail in ".:!?"
        lines[k] = line[: m.start()] + names(prose(line[m.start() :], first))
        if line[: m.start()].strip():
            tail = None
        else:
            body = line.rstrip()
            tail = body[-1] if body.strip("/#! ") else "."
    return "\n".join(lines)


def document(text):
    lines = text.split("\n")
    fenced = False
    for k, line in enumerate(lines):
        if line.lstrip().startswith("```"):
            fenced = not fenced
            continue
        before = lines[k - 1].rstrip() if k else ""
        first = not before or before[-1] in ".:!?|" or re.match(r"\s*([-*#|]|\d+\.)\s", line) is not None
        lines[k] = names(prose(line, first)) if not fenced else line
    return "\n".join(lines)


def put_right(rel, text):
    if rel.endswith(".rs"):
        return rust(text, rel)
    if rel.endswith((".md", ".txt")):
        return document(text)
    if rel.endswith((".sh", ".py", ".toml", ".yml", ".yaml", ".service", ".conf")) or "/" not in rel.replace("tools/", ""):
        return marked(text, "#")
    if rel.endswith((".qml", ".js", ".c", ".h", ".glsl", ".frag", ".vert")):
        text = marked(text, "//")
        return re.sub(r"/\*.*?\*/", lambda m: names(prose(m.group(0))), text, flags=re.S)
    return text
