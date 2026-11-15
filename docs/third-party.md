# Code taken from other projects

The workspace is `GPL-3.0-only` (`license` in `Cargo.toml`, the text in `LICENSE`). Crates
from crates.io are checked by `cargo deny` (`deny.toml`); this file lists what is copied or
ported into the tree itself.

| What | Where | From | License of the source | How it is used |
|---|---|---|---|---|
| The shell's QML, its scripts, icons and translations | `qml`; the scripts and QML the shell no longer loads, kept for the comparisons, in `crates/qml/tests/fixtures/original-unshipped` | [end-4/dots-hyprland](https://github.com/end-4/dots-hyprland), `dots/.config/quickshell/ii` | GPL-3.0 (its `LICENSE`), by end-4 and the contributors of that repository; a file of another origin names it and its license itself, as there | The interface of the shell, changed since: `docs/original-differences.md`. |
| The form and some lines of `README.md` | `README.md` | The same repository, `.github/README.md` | GPL-3.0 | Adapted to this project. |
| The grammar of Qt's QML parser | `tools/gen/qqmljs/qqmljs.g` | qtdeclarative v6.12.0, `src/qml/parser/qqmljs.g`, unchanged | `LicenseRef-Qt-Commercial OR LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only`, Copyright (C) 2016 The Qt Company Ltd. (the header of the file) | Taken under GPL-3.0-only. Input of `tools/gen/qqmljs/tables`. |
| The LALR tables of that grammar | `crates/qml/src/qqmljs/grammar.rs` | Output of Qt's `qlalr` for the file above | That of the grammar | Committed; a test checks they come from the committed grammar. |
| The parser driver, its error recovery and the actions of the grammar | `crates/qml/src/qqmljs/parser.rs`, `build.rs` | The C++ parts of the same `qqmljs.g`, ported to Rust | That of the grammar | Compiled into `qml`. |
| The lexer | `crates/qml/src/qqmljs/lexer.rs` | qtdeclarative v6.12.0, `src/qml/parser/qqmljslexer.cpp`, `qqmljskeywords_p.h`, ported to Rust | The same four licenses, Copyright (C) 2016 The Qt Company Ltd. | Compiled into `qml`. |
| The checks of QmlIR and its error texts | `crates/qml/src/qqmljs/ir.rs`, `build.rs` | qtdeclarative v6.12.0, `src/qml/compiler/qqmlirbuilder.cpp`; the list of global names from `qv4codegen.cpp` | The same four licenses, Copyright (C) 2016, 2017 The Qt Company Ltd. | Compiled into `qml`. |
| `regress` | `vendor/regress` | The `regress` crate, patched | See `vendor/regress` | `[patch.crates-io]`. |
| The icon of the shell's own windows (Settings, Welcome, the conflicts window) | `tools/install.sh` (`app_icon_text`) | Quickshell (its source at git.outfoxxed.me, commit `9b0f339b`), the file `quickshell.svg` in `assets`, unchanged; the desktop entry beside it (`app_entry_text`) follows the one in the same `assets/` | GNU LGPL 3 (its `README.md` and `LICENSE`), Copyright the Quickshell contributors | Taken under GPL-3.0-only (see below). An install writes it as `org.impulse-shell.svg` into the user's data directory, as Quickshell's package installs it under Quickshell's app id. |

Qt offers these files under any one of the four licenses. GPL-3.0-only is one of them and
is the license of this workspace, so the files and what is derived from them are distributed
under it; nothing of the commercial or the LGPL terms is relied on. No Qt code is linked:
the parser is Rust, and `qlalr` runs only by hand (`tools/gen/qqmljs/README.md`).

The LGPL 3 is the GPL 3 with additional permissions (LGPL 3, the first paragraph), and a
copy may be conveyed without them, under the GPL 3 alone (GPL 3, section 7): the icon is
distributed here under GPL-3.0-only, unchanged and with its origin named above and in
`tools/install.sh`.
