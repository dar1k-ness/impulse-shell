# The grammar of Qt's QML parser

`qqmljs.g` is `src/qml/parser/qqmljs.g` of qtdeclarative v6.12.0, unchanged, with its
license header. `crates/qml/src/qqmljs/grammar.rs` holds the LALR tables Qt's `qlalr` builds
from it, and `crates/qml/src/qqmljs` is the parser that runs on them: the lexer, the driver,
the error recovery and the actions of the grammar, ported to Rust.

The tables are committed. Nothing builds them on the way to a binary: no build script, no
image and no CI job has `qlalr`. To move to another Qt release, by hand:

1. Replace `qqmljs.g` with the file of that release.
2. Run `tools/gen/qqmljs/tables` on a machine with `qlalr` (Debian: `qt6-base-dev-tools`).
3. Carry over what changed in the actions of the grammar and in `qqmljslexer.cpp` to
   `crates/qml/src/qqmljs`: the rule numbers there are those of the tables.

The test `qqmljs::tests::tables_are_those_of_the_grammar` fails when `qqmljs.g` and the
tables do not belong together (the tables carry the SHA-256 of the grammar they came from).

License: see `docs/third-party.md`.
