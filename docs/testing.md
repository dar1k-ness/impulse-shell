# Test

Tests compare the shell with the original (Quickshell, the original's scripts, real services). The
reference tools are all in one image:

```sh
tools/ci/gate --workspace     # the full gate: what a pull request must pass before its merge
```

It needs only Docker: it builds the image (`tools/testenv/Containerfile`) on first use and
runs `tools/testenv/ri-gate` in it on this checkout. Steps, partial runs, timings, pre-commit
and the CI workflows: [docs/ci.md](ci.md).

`ri-gate` runs, for `qml` and `shell` (`ri-gate --workspace`: everything), these steps: the
tests under `cargo nextest` with `--features shell/interp` (`tests`), the `qml` test without
the interpreter (`no-interp`), the `rigen/check` workspace (`rigen`), `cargo fmt --check`
(`fmt`), clippy with `-D warnings` (`clippy`), a build and an install of the shipped shell
(`shipped-build`), its tests and the installed binary (`shipped-tests`), its clippy
(`shipped-clippy`), `cargo deny` (`deny`) and the shader precision tests on llvmpipe
(`gpu-precision`). With `--workspace` it also runs the strict AOT check (`aot`). The tests
are built with debug assertions on; there are no doctests.

One test binary, inside the image, from the same build as the gate's:

```sh
tools/ci/gate --only tests --test keyboard_focus
```

Outside the image a test whose reference tool is missing fails. `RI_EQUIV_SKIP=1` turns
that into a printed skip.

Rules the tests keep:

- A test waits on an event, never on the clock. `crates/shell/tests/lifecycle_waits_on_events.rs`
  scans the test code for sleeps, polls and timeouts and fails on one.
- The only guard against a hang is nextest's slow-timeout in `.config/nextest.toml`.
- Benchmarks and measurements against the host are not part of `cargo test`:
  [docs/benchmarks.md](benchmarks.md).

CI: `.github/workflows/checks.yml` (rustfmt, cargo-deny, clippy, unit tests, the shipped
shell, AOT check on pushes to `main`), `ri-gate.yml` (the full gate, started by hand) and
`gpu-precision.yml` (the shader precision tests, on pushes and pull requests that touch the
GPU code or the image). Only the last runs on pull requests: [docs/ci.md](ci.md).

## Diagnostics

`RI_LOG` selects log lines, `RI_DEBUG` sets debug switches. Every `II_*` / `RI_*` variable,
switch and log target is listed in [docs/env.md](env.md).

```sh
RI_LOG=ri::stats,ri::keys impulse-shell -c impulse-shell
RI_DEBUG=dmgcheck,laycheck impulse-shell -c impulse-shell
```
