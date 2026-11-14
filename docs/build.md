# Build

Needs:

- Rust 1.99.1. `rust-toolchain.toml` pins it; rustup installs it on first use.
- A C toolchain, `clang` (libclang, for the bindings to PipeWire that `bindgen` generates),
  `pkg-config`, `cmake`, `nasm`, `jq` (for `tools/install.sh`).
- Development files of Wayland, xkbcommon, EGL/GBM (Mesa), fontconfig, FreeType, HarfBuzz,
  libjpeg-turbo, PipeWire, PAM, libmagic. The exact Arch package list is the first `pacman`
  line of `tools/testenv/Containerfile`.
- At run time, opened when a file needs them: `libheif` for AVIF and HEIC, and `libavif`
  (1.0.0 to 1.4.2) for the colours of an AVIF wallpaper. Without `libavif`, or with a release
  outside that range, AVIF is read with libheif, whose pixels differ from the original scripts'
  by up to 83 levels at colour edges, and the log gets one line saying so.
- Optional at run time: `libthai` (`libthai.so.0`, the Arch package `libthai`), opened when a
  text has Thai in it, as Qt opens it. With it a Thai line is broken between the words of
  libthai's dictionary. Without it Thai is broken at spaces only, which is what Qt does
  without the library. `rusty-impulse --version` tells which of the two this machine gets
  (`Thai line breaking: ...`).
- `tools/install.sh` installs no packages. Of the libraries above it checks `libthai` alone:
  it asks the installed binary and prints a note when the library is missing; the install
  goes on.

```sh
tools/build.sh
```

It runs `cargo build --locked --release -p shell` and prints the path of the binary,
`target/release/rusty-impulse`. `RI_ROOT=<QML tree>` chooses the tree to compile (default `qml`).

One commit and one QML tree give the same file, byte for byte, wherever the checkout, Cargo's
home and the target directory are. `rusty-impulse --version` names the commit (`+changes` when a
file differs from it) and the checksum of the tree (`tools/build.sh --sum <tree>` prints it), and
the binary names none of the directories of the machine that built it: the script hides them
(`--remap-path-prefix`) and fails when one is left. A copy of the sources without `.git` cannot
tell its commit: `RI_COMMIT=<commit> tools/build.sh`, with what `tools/build.sh --commit` prints
in the checkout the copy was made from. Without a commit to name nothing is built,
and the release shell does not compile under plain `cargo build --release -p shell` either.

What still gives another file from the same commit: another Rust toolchain than
`rust-toolchain.toml`'s (rustup installs that one), another C compiler (`cc`, for the crates
that compile C), another linker, another version of a system library a crate reads at build
time (its headers or `pkg-config` data), other rustc flags (`RUSTFLAGS`, Cargo's configuration),
and another target (`--target`).

Build `-p shell` alone: together with other packages Cargo turns the interpreter on and the
release shell refuses to compile.
