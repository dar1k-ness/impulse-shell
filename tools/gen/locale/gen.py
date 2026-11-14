#!/usr/bin/env python3
"""Generate crates/qml/src/locale_data.rs from Qt's CLDR tables and, for a zone's names, the ICU
Qt asks: the Qt 6.12 of the tools/testenv image with the system's ICU (tools/testenv/system_qt.py)."""
import concurrent.futures
import json
import os
import subprocess
import sys
import unicodedata

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../tools/testenv"))
import system_qt  # the Qt that answers is the system's, checked there
from PySide6.QtCore import QLocale, QTimeZone

system_qt.checked()

F = QLocale.FormatType
recs, index, names = [], {}, {}


def rec(l):
    r = []
    for fmt in (F.LongFormat, F.ShortFormat, F.NarrowFormat):
        r += [l.dayName(d, fmt) for d in range(1, 8)]
        r += [l.standaloneDayName(d, fmt) for d in range(1, 8)]
        r += [l.monthName(m, fmt) for m in range(1, 13)]
        r += [l.standaloneMonthName(m, fmt) for m in range(1, 13)]
    r += [l.amText(), l.pmText()]
    for fmt in (F.LongFormat, F.ShortFormat):
        r += [l.dateFormat(fmt), l.timeFormat(fmt), l.dateTimeFormat(fmt)]
    r += numeric(l)
    r += zone(l)
    return tuple(r)


ZONE_IDS = sorted(bytes(i).decode() for i in QTimeZone.availableTimeZoneIds())
ZONES = [QTimeZone(i.encode()) for i in ZONE_IDS]
zone_names, zone_index = [], {}


def zone_tuple(l):
    """The standard and daylight-saving long names, then short names, of every zone in a locale
    (QTimeZone::displayName)."""
    t, n = QTimeZone.TimeType, QTimeZone.NameType
    return tuple(tuple(z.displayName(tt, nt, l) for nt in (n.LongName, n.ShortName)
                       for tt in (t.StandardTime, t.DaylightTime)) for z in ZONES)


def every_locale():
    return [QLocale.c()] + QLocale.matchingLocales(
        QLocale.Language.AnyLanguage, QLocale.Script.AnyScript, QLocale.Country.AnyCountry)


if sys.argv[1] == "--icu-default":
    # icu_names() below runs this, in a process whose environment names ICU's default locale,
    # with a locale ICU has no data for.
    json.dump(zone_tuple(QLocale(sys.argv[2])), sys.stdout)
    sys.exit(0)
if sys.argv[1] == "--icu-probe":
    # no_icu_data() below runs this: one zone's long name in every locale.
    t, n = QTimeZone.TimeType, QTimeZone.NameType
    india = QTimeZone(b"Asia/Kolkata")
    json.dump([india.displayName(t.StandardTime, n.LongName, l) for l in every_locale()], sys.stdout)
    sys.exit(0)
ICU_VARS = [k for k in ("LC_ALL", "LC_MESSAGES", "LANG") if k in os.environ]
if ICU_VARS:
    # A zone's names in a locale ICU has no data for are in ICU's default locale, which those
    # three name: the file is written with none of them, where that is en_US_POSIX.
    os.execve(sys.executable, [sys.executable] + sys.argv,
              {k: v for k, v in os.environ.items() if k not in ICU_VARS})


def key(l):
    return (l.language(), l.script(), l.territory())


def zone_row(v):
    """The index of the names `v` (a zone_tuple) in `zone_names`."""
    if v not in zone_index:
        zone_index[v] = len(zone_names)
        zone_names.append(v)
    return zone_index[v]


def icu(lang, *mode):
    """What this script prints with `mode` in a process where LANG is `lang` and nothing else
    names a locale. ICU fixes its default locale the first time a process asks, from the
    environment (uprv_getDefaultLocaleID, putil.cpp), so another process answers."""
    return json.loads(subprocess.run([sys.executable, __file__, *mode], env={**os.environ, "LANG": lang},
                                     check=True, stdout=subprocess.PIPE).stdout)


def no_icu_data():
    """The locales ICU has no data for under QLocale::name() (`C`, `iu_CA`): a zone's name in one
    is in ICU's default locale, so it is another with another LANG."""
    names = zip(every_locale(), icu("", "--icu-probe"), icu("de_DE", "--icu-probe"))
    return {key(l) for l, root, german in names if root != german}


no_icu = no_icu_data()
icu_asked = {}


def icu_names(lang):
    """The names (a zone_tuple) ICU has in its default locale when LANG is `lang`: those Qt gives
    there in a locale ICU has no data for, one of another language than `lang`, which ICU would
    take for the default one (findFirstExisting, uresbund.cpp)."""
    if lang not in icu_asked:
        other = QLocale("la_VA" if lang.lower().startswith("iu") else "iu_CA")
        assert key(other) in no_icu, "ICU has data for %s: name another locale it has none for" % other.name()
        icu_asked[lang] = tuple(tuple(v) for v in icu(lang, "--icu-default", other.name()))
    return icu_asked[lang]


def is_anglic(l):
    """Whether the locale is the C locale or English in Latin script: for those Qt takes a zone's
    short name from the zone's own data, its abbreviation at the time asked about."""
    return l.language() == QLocale.Language.C or (
        l.language() == QLocale.Language.English and l.script() == QLocale.Script.LatinScript)


def zone(l):
    """The index of the locale's zone names in `zone_names`, then whether it is_anglic. The names
    are ICU's, which Qt asks by QLocale::name(): those of the default locale ICU has here,
    en_US_POSIX, for a locale ICU has no data for. A short name ICU has in English is no name
    of Qt's in that locale, and is one in a locale ICU has no data for where ICU's default
    locale is that English one."""
    english = is_anglic(l) and l.language() != QLocale.Language.C
    return [str(zone_row(icu_names(l.name()) if english else zone_tuple(l))), "1" if is_anglic(l) else ""]


def names_another(l):
    """Whether QLocale::name() of the locale names another one (sr_Latn_RS is `sr_RS`, which is
    sr_Cyrl_RS): it has no script."""
    return l.language() != QLocale.Language.C and QLocale(l.name()) != l


def script_name(l):
    """The locale's name with its script, as ICU has it: `sr_Latn_RS`."""
    tags = QLocale.languageToCode(l.language()), QLocale.scriptToCode(l.script()), QLocale.territoryToCode(l.territory())
    return "_".join(tags)


def grouping(l):
    """QLocaleData's (first, higher, least) digit-group sizes, read back from grouped integers."""
    g = QLocale(l)
    g.setNumberOptions(QLocale.NumberOption.DefaultNumberOptions)
    sep = g.groupSeparator()
    parts = [g.toString(10 ** (n - 1)).split(sep) for n in range(1, 19)]
    n0 = next(n for n, p in enumerate(parts, 1) if len(p) > 1)
    least = len(parts[n0 - 1][-1])
    first = n0 - least
    p = parts[-1]
    higher = len(p[-2]) if len(p) > 2 else 18
    for n, p in enumerate(parts, 1):  # QLocaleData::decimalForm's grouping, checked against Qt
        cuts, i = [], n - least
        if i >= first:
            cuts.append(i)
            while (i := i - higher) > 0:
                cuts.append(i)
        assert [len(x) for x in p] == [b - a for a, b in zip([0] + cuts[::-1], cuts[::-1] + [n])], l.name()
    return first, higher, least


def numeric(l):
    """Number and currency formatting data (QLocale::toString(double) / toCurrencyString)."""
    first, higher, least = grouping(l)
    sym, num = "\ue000", l.toString(1.5, "f", 1)

    def fmt(s):
        assert s.count(num) == 1 and s.count(sym) <= 1, l.name()
        return s.replace(num, "%1").replace(sym, "%2")

    def apply(f, n):
        return f.replace("%1", n).replace("%2", sym)

    pos = fmt(l.toCurrencyString(1.5, sym, 1))
    neg_r, nan_r = l.toCurrencyString(-1.5, sym, 1), l.toCurrencyString(float("nan"), sym, 1)
    neg = ""
    if neg_r != apply(pos, l.toString(-1.5, "f", 1)) or nan_r != apply(pos, "nan"):
        neg = fmt(neg_r)
        assert nan_r == apply(neg, "nan"), l.name()
    digits = next(k for k in range(7) if l.toCurrencyString(1.5, sym) == l.toCurrencyString(1.5, sym, k))
    opts = int(l.numberOptions().value) if hasattr(l.numberOptions(), "value") else int(l.numberOptions())
    return [l.decimalPoint(), l.groupSeparator(), l.negativeSign(), l.positiveSign(), l.exponential(),
            l.zeroDigit(), f"{first},{higher},{least}",
            l.currencySymbol(QLocale.CurrencySymbolFormat.CurrencySymbol),
            l.currencySymbol(QLocale.CurrencySymbolFormat.CurrencyIsoCode), pos, neg, str(digits), str(opts),
            "1" if l.script() == QLocale.Script.CyrillicScript else ""]


def patterns(l):
    """QLocale::createSeparatedList's four patterns (two items; start, middle, end of a longer
    list), read back from lists of private-use items and checked against Qt up to six items."""
    it = [chr(0xE000 + i) for i in range(6)]

    def pat(s, a, b):
        assert s.count(a) == 1 and s.count(b) == 1, l.name()
        return s.replace(a, "%1").replace(b, "%2")

    two = pat(l.createSeparatedList(it[:2]), it[0], it[1])
    three, four = l.createSeparatedList(it[:3]), l.createSeparatedList(it[:4])
    head = three[three.index(it[0]):three.index(it[1]) + 1]
    start = pat(head, it[0], it[1])
    end = pat(three.replace(head, "\ue010"), "\ue010", it[2])
    e0, e1 = end.index("%1"), len(end) - end.index("%2") - 2
    mid_all = four.replace(head, "\ue010")
    cut = mid_all.index(it[2]) + 1
    assert mid_all[:e0] == end[:e0] and mid_all[cut:] == end[e0 + 2:].replace("%2", it[3]), l.name()
    mid = pat(mid_all[e0:cut], "\ue010", it[2])

    def join(xs):
        if len(xs) < 2:
            return "".join(xs)
        if len(xs) == 2:
            return two.replace("%1", xs[0]).replace("%2", xs[1])
        r = start.replace("%1", xs[0]).replace("%2", xs[1])
        for x in xs[2:-1]:
            r = mid.replace("%1", r).replace("%2", x)
        return end.replace("%1", r).replace("%2", xs[-1])

    for n in range(7):
        assert join(it[:n]) == l.createSeparatedList(it[:n]), l.name()
    return [two, start, mid, end]


def units(l):
    """The byte unit names of QLocale::formattedDataSize: bytes, six SI and six IEC quantifiers."""
    D = QLocale.DataSizeFormat

    def unit(n, f):
        s = l.formattedDataSize(n, 0, f)
        return s[s.index(" ") + 1:]

    return ([unit(1, D.DataSizeIecFormat)] + [unit(1000 ** k, D.DataSizeSIFormat) for k in range(1, 7)]
            + [unit(1024 ** k, D.DataSizeIecFormat) for k in range(1, 7)])


def identity(l):
    """What tells one locale from another, and the rest of the QML Locale type's properties."""
    us = units(l)
    assert not any("\x1e" in u for u in us)
    return [l.name(), QLocale.languageToString(l.language()), QLocale.scriptToString(l.script()),
            QLocale.territoryToString(l.territory()), l.nativeLanguageName(), l.nativeTerritoryName(),
            l.percent(), str(l.textDirection().value), str(l.measurementSystem().value),
            str(l.firstDayOfWeek().value % 7), ",".join(str(d.value % 7) for d in l.weekdays()),
            "|".join(l.uiLanguages()), l.currencySymbol(QLocale.CurrencySymbolFormat.CurrencyDisplayName),
            *patterns(l), "\x1e".join(us)]


def add(l):
    k = key(l)
    if k not in index:
        index[k] = len(recs)
        recs.append(tuple(rec(l) + tuple(identity(l)) + ("1" if k in no_icu else "",)))
    return index[k]


locs = every_locale()[1:]
# The names ICU has with one LANG are asked of a process each: all at once here.
with concurrent.futures.ThreadPoolExecutor(os.cpu_count()) as pool:
    list(pool.map(icu_names, [""] + [l.name() for l in locs if is_anglic(l)] + [
        script_name(l) for l in locs if names_another(l)]))
add(QLocale.c())
for l in locs:
    add(l)

# Language codes: every two- and three-letter code QLocale::codeToLanguage knows, mapped to the
# code QLocale names the language by.
letters = "abcdefghijklmnopqrstuvwxyz"
codes = [a + b for a in letters for b in letters]
codes += [c + d for c in codes for d in letters]
langs = {}
for c in codes:
    lang = QLocale.codeToLanguage(c)
    if lang not in (QLocale.Language.AnyLanguage, QLocale.Language.C):
        langs[c] = QLocale.languageToCode(lang)
scripts = sorted({QLocale.scriptToCode(x) for x in QLocale.Script} - {""})
territories = sorted({QLocale.territoryToCode(l.territory()) for l in locs} - {""})

# Names: QLocale(name) for a language alone and with a script, a territory or both. Rust tries
# `l_s_t`, `l_s`, `l_t`, `l` in that order and falls back to C; an entry is written for every
# name of a locale and for every other name that order would get wrong.
for l in locs:
    code = QLocale.languageToCode(l.language())
    s, t = QLocale.scriptToCode(l.script()), QLocale.territoryToCode(l.territory())
    for n in (code, f"{code}_{s}", f"{code}_{t}", f"{code}_{s}_{t}"):
        names.setdefault(n, add(QLocale(n)))


def lookup(l, s, t):
    for n in ([f"{l}_{s}_{t}"] if s and t else []) + ([f"{l}_{s}"] if s else []) + ([f"{l}_{t}"] if t else []) + [l]:
        if n in names:
            return names[n]
    return 0


extra = 0
for code in sorted(set(langs.values())):
    for s in [""] + scripts:
        for t in [""] + territories + ["ZZ"]:
            n = "_".join(x for x in (code, s, t) if x)
            want = index[key(QLocale(n))]
            if lookup(code, s, t) != want:
                names[n] = want
                extra += 1
print(extra, "names beside the locales' own", file=sys.stderr)


# The names of ICU's root locale (`GMT+05:30`): what an empty LANG leaves ICU with.
root = zone_row(icu_names(""))
# QLocale::name() has no script, so for a locale whose script is not the one its language has in
# its territory it names another locale (sr_Latn_RS is `sr_RS`, which is sr_Cyrl_RS), and Qt
# gives the zone names of that one. ICU's default locale keeps the script LANG names.
script_zone = {index[key(l)]: zone_row(icu_names(script_name(l))) for l in locs if names_another(l)}
# Rust takes a name with a script for one ICU may have data for when it is in `names`.
script_of = {index[key(l)]: QLocale.scriptToCode(l.script()) for l in locs}
for n, r in names.items():
    tags = n.split("_")
    assert len(tags) < 2 or tags[1] not in scripts or script_of[r] == tags[1], n


def lit(s):
    """A Rust string literal; control and invisible format characters (ZWSP, LRM, ...) escaped."""
    s = s.replace("\\", "\\\\").replace('"', '\\"')
    return '"' + "".join(f"\\u{{{ord(c):x}}}" if unicodedata.category(c) in ("Cc", "Cf") else c for c in s) + '"'


out = ["// @generated by tools/gen/locale/gen.py from Qt 6.12 (CLDR) QLocale data. Do not edit.",
       "// Record (fields joined by U+001F): per Long/Short/Narrow: 7 days (Mon..Sun), 7 standalone days, 12 months,",
       "// 12 standalone months; then am, pm; per Long/Short: date, time, dateTime formats; then decimal point,",
       "// group separator, negative sign, positive sign, exponential, zero digit, grouping (first,higher,least),",
       "// currency symbol, currency ISO code, currency format, negative currency format (empty: none), currency",
       "// digits, QLocale::NumberOptions, Cyrillic script (\"1\" or empty), index into ZONE_NAMES,",
       "// C or English in Latin script (\"1\" or empty); then name, English language, script",
       "// and territory names, native language and territory names, percent, text direction, measurement",
       "// system, first day of the week and week days (Sunday = 0), UI languages (joined by |), currency",
       "// display name, list patterns (two; start, middle, end), byte units (bytes, 6 SI, 6 IEC; joined by",
       "// U+001E); then ICU has no data for the name (\"1\" or empty). One record per locale (language,",
       "// script, territory).",
       f"pub(crate) const RECORD_LEN: usize = {len(recs[0])};",
       f"pub(crate) static RECORDS: [&str; {len(recs)}] = ["]
for r in recs:
    assert not any("\x1f" in s for s in r)
    out.append("    " + lit("\x1f".join(r)) + ",")
out.append("];")
out.append("/// Locale name (`l`, `l_Scri`, `l_TT`, `l_Scri_TT`) -> RECORDS index, sorted by name.")
out.append(f"pub(crate) static NAMES: [(&str, u16); {len(names)}] = [")
for n in sorted(names):
    out.append(f"    ({lit(n)}, {names[n]}),")
out.append("];")
# Zones with the same names in every locale share one entry of a ZONE_NAMES row.
groups, group_of = [], {}
for k in range(len(ZONE_IDS)):
    key = tuple(v[k] for v in zone_names)
    if key not in group_of:
        group_of[key] = len(groups)
        groups.append(k)
out.append("/// IANA zone id -> index of the zone's four names in a ZONE_NAMES row, sorted by id.")
out.append(f"pub(crate) static ZONES: [(&str, u16); {len(ZONE_IDS)}] = [")
for k, i in enumerate(ZONE_IDS):
    out.append(f"    ({lit(i)}, {group_of[tuple(v[k] for v in zone_names)]}),")
out.append("];")
out.append("/// Per group of locales, four names per zone index (joined by `|`): standard and daylight-saving long")
out.append("/// name, standard and daylight-saving short name (QTimeZone::displayName; empty: Qt has none).")
out.append(f"pub(crate) static ZONE_NAMES: [&str; {len(zone_names)}] = [")
for v in zone_names:
    row = [s for k in groups for s in v[k]]
    assert not any("|" in s for s in row)
    out.append("    " + lit("|".join(row)) + ",")
out.append("];")
out.append("/// The ZONE_NAMES row of ICU's root locale: a zone's offset from GMT, or `GMT`.")
out.append(f"pub(crate) const ROOT_ZONE_NAMES: usize = {root};")
out.append("/// RECORDS index of a locale whose name (QLocale::name(), no script) is another locale's ->")
out.append("/// the ZONE_NAMES row ICU has for its name with the script (`sr_Latn_RS`); sorted.")
out.append(f"pub(crate) static SCRIPT_ZONE_NAMES: [(u16, u16); {len(script_zone)}] = [")
out += [f"    ({r}, {z})," for r, z in sorted(script_zone.items())]
out.append("];")
out.append("/// Language code -> the code QLocale names that language by, where they differ; sorted.")
alias = sorted((c, p) for c, p in langs.items() if c != p)
out.append(f"pub(crate) static LANGUAGES: [(&str, &str); {len(alias)}] = [")
out += [f"    ({lit(c)}, {lit(p)})," for c, p in alias]
out.append("];")
out.append("/// QLocale's script codes, sorted.")
out.append(f"pub(crate) static SCRIPTS: [&str; {len(scripts)}] = [")
# Short items packed into lines of at most 100 columns, the way rustfmt lays an array out.
line = "   "
for x in scripts:
    if len(line) + 1 + len(lit(x)) + 1 > 100:
        out.append(line)
        line = "   "
    line += " " + lit(x) + ","
out.append(line)
out.append("];")
open(sys.argv[1], "w").write("\n".join(out) + "\n")
print(len(recs), "records", len(names), "names", file=sys.stderr)
