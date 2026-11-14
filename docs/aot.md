# AOT: the original's JavaScript compiled to Rust

The original (rusty-impulse) is written in QML with JavaScript in its bindings, handlers and
functions and in 6 `.js` libraries. The installed shell does not interpret that JavaScript.
At build time every script is compiled to a Rust function; at run time the shell looks the
function up by a key and calls it. The QML markup itself is still read from the QML tree at
startup.

## Pipeline

```
qml/*.qml, *.js
  -> qml parser, qml::ir (lowering to IR)            build time only, feature `interp`
  -> crates/rigen (IR -> Rust source)                run by crates/ricompiled-gen/build.rs
  -> crates/ricompiled/parts/p00..p31                the generated code, 32 crates
  -> crates/ricompiled (registry: key -> function)   linked into rusty-impulse
```

- The tree compiled is `RI_ROOT` (default `qml`, the shell's QML in this repository).
- The generated code is split over 32 part crates so that rustc's memory stays bounded;
  `crates/ricompiled/parts/mkparts.sh` regenerates them.
- Generated code calls only `qml::jsrt` (the ABI: `CompiledFn`, `Env`, `Entry`) and the value
  operations of `qml::eval` and `qml::builtins`.

## Text changed before the JavaScript parser

One place rewrites JavaScript text before oxc parses it.

- What: in `for (async of xs)` the word `async` is passed to oxc as `\u0061sync`
  (`qml::qqmljs::for_async_of`, called by `qml::ir` for bindings, handlers, functions, `.js`
  files, `eval` and `Function(...)`, and by `qml::js::check`). It finds the three tokens
  `for`, `(`, `async` followed by the token `of`, as Qt's lexer and parser read the text
  (`crates/qml/src/qqmljs`): comments and line breaks between them do not matter, and the
  same text inside a string, a template, a regular expression or a comment is left alone.
- Why: the JavaScript of QML in Qt 6.12 has no async functions, so `async` is a plain name
  there and the loop assigns to it. ECMAScript forbids these tokens (they could start
  `for (async of => ...;;)`), oxc follows ECMAScript and has no option for it; an escaped
  name is outside that rule and is the same name.
- Not visible outside: positions in syntax errors come from the QML parser, which reads the
  text as written; the IR has no positions and no source text, so nothing is shifted by the
  five extra characters. If the IR ever takes positions from oxc, the columns right of a
  rewritten `async` on its line must be moved back by 5.
- Tests: `for_async_of_assigns_the_name_async` and `an_error_after_for_async_of_has_its_column`
  in `crates/qml/tests/engine_audit_parser.rs`, and `crates/rigen/check/cases/qml/for_async_of.qml`
  for the compiled code; each expects what Qt 6.12 gives.

## Keys

`qml::walk` gives every script its key:

- QML script: `(file, object path, member name)`, e.g. file `modules/bar/Bar.qml`, object
  path `0.3.1` (root is `0`, owned objects are numbered in walk order), member
  `anchors.fill` / `onClicked` / a function name.
- `.js` library: its path relative to the original root.

Each registry entry also stores a hash of the script source. A lookup succeeds only when
the source at run time hashes to what was compiled.

## What the build refuses

`crates/ricompiled-gen/build.rs` fails the build when the tree has

- no `shell.qml` (no tree at `RI_ROOT`),
- a QML type the shell does not implement,
- a JavaScript construct `rigen` cannot compile,
- `Qt.createQmlObject` (QML text built at run time), or `eval` or `Function(...)` of JavaScript text.
  `Function` counts when it is called, taken as a value (`var F = Function`) or reached as
  `.constructor` of a function written in place. The constructor of a function held in a
  variable (`f.constructor("...")`) is not visible to the build: in the shipped shell that call
  throws `EvalError`.

## What the installed shell refuses

`rusty-impulse` is built without the `interp` feature: no parser that makes a tree of JavaScript, no
IR, no tree-walking evaluator. `crates/shell/src/main.rs` fails to compile a release shell
that got `qml/interp` through Cargo's feature unification.

- At startup the shell compares the QML tree it was asked to run with the hashes compiled in
  (`ricompiled::stale_files`). If a file differs it prints the files and
  `tools/install.sh` (which puts the repository's tree back), and exits with status 1.
- `Rustshell.reload()` does the same check first and keeps the current shell running when
  the tree changed.
- `rusty-impulse --check-tree -c rusty-impulse` (or `-p <tree>`) does only that check: status 0 if the tree is
  the one compiled in, else the changed and the missing files and status 1.
  `tools/install.sh` asks it before an install or a rollback replaces anything.

So after every change of the tree the shell has to be built and installed again.

Cargo sees that the tree changed by its file times only (Cargo Book, build scripts,
`rerun-if-changed`). `tools/build.sh` (which `tools/install.sh` runs) also gives the build a
checksum of the tree's content (`RI_ROOT_SUM`, `rerun-if-env-changed`), so it compiles a tree
whose files are older than the last build as well. A plain `cargo build` keeps its build of
such a tree, and that binary then refuses the tree at startup.

## Where QML is parsed at run time

The QML parser (`crates/qml/src/qqmljs`: Qt 6.12's grammar and lexer, `docs/third-party.md`)
is in every build. It accepts and rejects a document as Qt does, the JavaScript in it
included, and keeps that JavaScript as text. The installed shell runs it

- once for each QML file it loads (`Eng::doc`, which keeps the parsed document): the tree at
  startup, and a file a `Loader`, `Qt.createComponent` or a type name reaches later. A file
  with an error gives `Component.errorString()` its lines `url:line:column: message`;
- for the text given to `Qt.createQmlObject`, whose error is thrown as Qt's
  `Qt.createQmlObject(): failed to create object:` with the same lines;
- for a file of the tree that differs from the one compiled in, to say what differs
  (`ricompiled::stale_files`, also before `Rustshell.reload()`).

Parsing all 624 files of `qml/` (1.9 MB) takes about 60 ms on the machine where the parser
before this one, which only skipped over JavaScript, took 30 ms.

## The interpreter

The tree-walking interpreter still exists behind the `interp` feature of `qml` (default on
for `qml`, off for `shell`). It is what the compiled code is checked against. It is used by
tests, `aotcheck`, `rigen` and benchmarks. A shell built with `--features interp` runs
compiled code too; `RI_INTERP=1` makes it interpret every script.

## Native types

Some things the QML uses are not JavaScript at all: they are types of the `Impulse` module,
written in Rust and named in the QML like any other type. See
[native-types.md](native-types.md).

## Checks

| Check | What it does | Where it runs |
| --- | --- | --- |
| `crates/rigen/check` | Its own workspace. Compiles JS snippets (and, with `RI_DIR`, every original script) and runs them against the interpreter. | `ri-gate` (step `rigen`) |
| `riaotcheck qml --backend compiled --strict --v4 <python>` | `crates/aotcheck` with the compiled code as candidate: runs the whole shell headless, interpreter and compiled side by side, and compares the world state after each step. `--strict` fails on any divergence, interpreted script, unsupported type or script error raised while a scene loads or its panels open and close (`ERROR` lines). `--v4` replays sampled bindings in Qt's QV4. | `ri-gate --workspace` (step `aot`), `checks.yml` |
| `cargo nextest run -p qml --no-default-features --test no_interp` | The qml crate without the interpreter. | `ri-gate` (step `no-interp`), `checks.yml` |
| `cargo nextest run -p shell --test startup_errors` | The shell without the interpreter refuses a changed tree. | `ri-gate` (step `shipped-tests`, which runs every shell test without the interpreter), `checks.yml` (job `unit-tests`) |
| `crates/testscenes` | A test whose QML is not in the shell's tree keeps it as a file under `crates/shell/tests/scenes` and loads it with `testscenes::load`. The crate's `build.rs` compiles that directory with `rigen` into tables of its own; only tests depend on it, so the shell's binary holds none of it. With the interpreter the scene is interpreted, without it its compiled code runs; a scene the build did not compile fails the test, and a scene named as a file of the shell's tree fails the build. | `ri-gate` (`tests`, `shipped-tests`) |
