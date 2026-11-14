# Licences of the syntax definitions and themes

Everything in this directory is data of KDE's KSyntaxHighlighting, copied unchanged from
one version of it:

- project: <https://invent.kde.org/frameworks/syntax-highlighting>
- version: 6.31.0, tag `v6.31.0`, commit `6d8ab7945958e7354295db7f399cce06f7de7571`
- `syntax/*.xml` from its `data/syntax/`, `themes/*.theme` from its `data/themes/`

One file is not in that directory of the project: `syntax/doxygenlua.xml`. The project
builds it from `data/syntax/doxygen.xml` with `data/generators/generate-doxygenlua.pl`
(`data/CMakeLists.txt` runs the script at build time), and `lua.xml` includes it. The copy
here is the output of that script of the same commit, run on the same `doxygen.xml`.

Only the data is used. No code of KSyntaxHighlighting, Qt or KDE is built into rusty-impulse: the
files are read by `crates/qml/src/syntax/kate.rs`.

The texts of the licences are beside this file: `MIT.txt` and `LGPL-2.0-or-later.txt`, both
copied from the project's `LICENSES/` of the same commit. The GNU General Public License is
the workspace's own `LICENSE`.

## Each file

"In the file" is the `license` attribute of the file's `<language>` element, as written
there. "debian/copyright" is the licence Debian's `kf6-syntax-highlighting` 6.30.0-1 records
for the file; `Files: *` marks a file that entry does not list by name, which falls under
its first paragraph.

| File | Definition | Version | Used as | In the file | debian/copyright 6.30.0-1 |
|---|---|---|---|---|---|
| `alert.xml` | Alerts | 8 | included by others | `MIT` | `MIT` |
| `bash.xml` | Bash | 58 | language | `LGPL` | `LGPL-2.0-only` |
| `c.xml` | C | 19 | language | none | `MIT` (`Files: *`) |
| `cmake.xml` | CMake | 61 | language | `LGPLv2+` | `LGPL-2.0-or-later` |
| `comments.xml` | Comments | 1 | included by others | `MIT` | `MIT` |
| `cpp.xml` | C++ | 21 | language | `LGPL` | `LGPL-2.0-only` |
| `cs.xml` | C# | 14 | language | none | `MIT` (`Files: *`) |
| `css.xml` | CSS | 17 | language | `LGPL` | `LGPL-2.0-only` |
| `diff.xml` | Diff | 9 | language | none | `MIT` (`Files: *`) |
| `dockerfile.xml` | Dockerfile | 11 | language | `MIT` | `MIT` (`Files: *`) |
| `doxygen.xml` | Doxygen | 15 | included by others | `MIT` | `MIT` |
| `doxygenlua.xml` | DoxygenLua | 17 | included by others | `MIT` | generated from `doxygen.xml` (MIT) |
| `gcc.xml` | GCCExtensions | 5 | included by others | `LGPL` | `LGPL-2.0-only` |
| `go.xml` | Go | 10 | language | `GPLv2+` | `GPL-2.0-or-later` |
| `html.xml` | HTML | 20 | language | `LGPL` | `LGPL-2.0-only` |
| `isocpp.xml` | ISO C++ | 32 | included by others | `LGPL` | `LGPL-2.0-only` |
| `java.xml` | Java | 13 | language | `LGPL` | `LGPL-2.0-only` |
| `javadoc.xml` | Javadoc | 7 | included by others | `LGPL` | `LGPL-2.0-only` |
| `javascript-react.xml` | JavaScript React (JSX) | 11 | included by others | `MIT` | `MIT` |
| `javascript.xml` | JavaScript | 26 | language | none | `MIT` |
| `json.xml` | JSON | 10 | language | `GPL` | `GPL` |
| `kotlin.xml` | Kotlin | 12 | language | `LGPLv2+` | `LGPL-2.0-or-later` |
| `latex.xml` | LaTeX | 30 | language | `LGPL` | `LGPL-2.0-only` |
| `lua.xml` | Lua | 21 | language | none | `MIT` (`Files: *`) |
| `makefile.xml` | Makefile | 17 | language | none | `MIT` |
| `markdown.xml` | Markdown | 33 | language | `GPL,BSD` | `GPL_OR_BSD_with_parts_under_MIT` |
| `modelines.xml` | Modelines | 7 | included by others | `MIT` | `MIT` |
| `mustache.xml` | Mustache/Handlebars (HTML) | 10 | included by others | `MIT` | `MIT` |
| `nix.xml` | Nix | 5 | language | `MIT` | `MIT` (`Files: *`) |
| `python.xml` | Python | 33 | language | none | `MIT` |
| `qml.xml` | QML | 14 | language | `MIT` | `MIT` |
| `rest.xml` | reStructuredText | 10 | included by others | none | `MIT` (`Files: *`) |
| `ruby.xml` | Ruby | 21 | language | `LGPLv2+` | `MIT` (`Files: *`) |
| `rust.xml` | Rust | 16 | language | `MIT` | `MIT` |
| `spdx-comments.xml` | SPDX-Comments | 7 | included by others | `MIT` | `MIT` |
| `sql.xml` | SQL | 9 | language | `LGPL` | `LGPL-2.0-only` |
| `swift.xml` | Swift | 2 | language | `MIT` | `MIT` |
| `toml.xml` | TOML | 13 | language | `LGPLv2+` | `LGPL-2.0-or-later` |
| `typescript.xml` | TypeScript | 18 | language | `MIT` | `MIT` |
| `xml.xml` | XML | 27 | language | `LGPL` | `LGPL-2.0-only` |
| `yaml.xml` | YAML | 18 | language | `LGPL` | `LGPL-2.0-only_with_parts_under_MIT` |

| File | Theme | Revision | In the file |
|---|---|---|---|
| `themes/monokai.theme` | Monokai | 9 | `SPDX-License-Identifier: MIT`; `SPDX-FileCopyrightText: 2006 Wimer Hazenberg`, `SPDX-FileCopyrightText: 2020 Waqar Ahmed <waqar.17a@gmail.com>` |
| `themes/ayu-light.theme` | ayu Light | 4 | `SPDX-License-Identifier: MIT`; `SPDX-FileCopyrightText: 2016 Ike Ku`, `SPDX-FileCopyrightText: 2020 Nibaldo González <nibgonz@gmail.com>` |

How each licence goes with the workspace's GPL-3.0-only:

- MIT: used under MIT; its notice is `MIT.txt`.
- `LGPL`, `LGPLv2+`: used under the GNU General Public License version 3, as section 3 of
  the GNU Library General Public License version 2 (`LGPL-2.0-or-later.txt`) allows for any
  copy.
- `GPLv2+` (`go.xml`): used under version 3.
- `GPL` (`json.xml`): names no version, so any version may be chosen (section 9 of version
  2); used under version 3.
- `GPL,BSD` (`markdown.xml`): "Dual-Licensed under both GPL and BSD licenses" in the file's
  header, with later changes under MIT; used under the GPL, version 3.
- `yaml.xml`: `LGPL`, with modifications its header puts under MIT.

Where the file and Debian differ (`ruby.xml`: `LGPLv2+` in the file, MIT by `Files: *` in
Debian), the file is followed.

## The files that state no licence

Eight files have no licence in them: `c.xml`, `cs.xml`, `diff.xml`, `lua.xml` (the
attribute is absent), `javascript.xml`, `makefile.xml`, `python.xml` (the attribute is
empty) and `rest.xml` (absent). They are used under MIT, on these two sources, quoted word
for word.

### The project's rule

`README.md` of KSyntaxHighlighting 6.31.0 (commit `6d8ab7945958e7354295db7f399cce06f7de7571`),
section "Licensing":
<https://invent.kde.org/frameworks/syntax-highlighting/-/blob/v6.31.0/README.md#licensing>

> ### Licensing
>
> Contributions to KSyntaxHighlighting shall be licensed under [MIT](LICENSES/MIT.txt).
>
> All files shall contain a proper "SPDX-License-Identifier: MIT" identifier inside a header like:
>
> ```cpp
> /*
>     SPDX-FileCopyrightText: 2020 Christoph Cullmann <cullmann@kde.org>
>
>     SPDX-License-Identifier: MIT
> */
> ```

### Debian's record

`debian/copyright` of the Debian package `kf6-syntax-highlighting` 6.30.0-1:
<https://sources.debian.org/data/main/k/kf6-syntax-highlighting/6.30.0-1/debian/copyright>

`javascript.xml`, `makefile.xml` and `python.xml` are listed by name in this paragraph (the
list of files and of copyright holders is shortened here with `[...]`; the lines shown are
as in the file):

```
Files: data/generators/generate-cmake-syntax.py
       [...]
       data/syntax/javascript.xml
       [...]
       data/syntax/makefile.xml
       [...]
       data/syntax/python.xml
       [...]
Copyright: Aaron Puchert
           [...]
           2002, Anders Lund <anders@alweb.dk>
           Anders Lund <anders@alweb.dk>
           [...]
           2001, Joseph Wenninger <jowenn@kde.org>
           Joseph Wenninger <jowenn@kde.org>
           [...]
           Michael Bueker
           [...]
           Per Wigren <wigren@home.se>
           [...]
           Whitehawk Stormchaser <zerokode@gmx.net>
           [...]
License: MIT
Comment: Manually collected
```

`c.xml`, `cs.xml`, `diff.xml`, `lua.xml` and `rest.xml` are not listed by name in any
paragraph, so the first one covers them (copyright holders shortened with `[...]`):

```
Files: *
Copyright: 2021, Alberto Salvia Novella <https://es20490446e.wordpress.com>
           [...]
           2014-2020, Christoph Cullmann <cullmann@kde.org>
           [...]
           2012-2018, Dominik Haumann <dhaumann@kde.org>
           [...]
           2016-2022, Volker Krause <vkrause@kde.org>
           [...]
License: MIT
Comment: Automatically extracted
```

The record is of version 6.30.0; the eight files are taken from 6.31.0 like the rest.
