# Install

```sh
tools/install.sh              # build, then install to /usr/local/bin/rusty-impulse
PREFIX=~/.local tools/install.sh
NOBUILD=1 tools/install.sh    # install the existing build (the one in Cargo's target directory)
BIN=<file> tools/install.sh   # install an rusty-impulse built elsewhere
tools/install.sh --rollback   # back to the release the last install replaced
tools/install.sh --status     # the current and the previous release, and the rusty-impulse config
tools/install.sh --uninstall
```

- It compiles this checkout's `qml` (`RI_ROOT` overrides) and makes the rusty-impulse config
  `~/.config/rustshell/rusty-impulse`, what `rusty-impulse -c rusty-impulse` runs, a copy of it, with the files'
  executable bits. A config that was there with other content is kept as
  `~/.local/state/rusty-impulse/replaced-<time>.<random>`, a link in it as the link; the script never
  removes these copies. Run it as the user whose shell it is, not as root (`sudo`, `doas`,
  `pkexec`, `su`).
- It uses `sudo` only when the target directories are not writable.
- The binary and a copy of the QML tree it was compiled from are kept together as a release in
  `$PREFIX/lib/rusty-impulse/releases/<id>`; `$PREFIX/bin/rusty-impulse` is a link to the current one. The
  current and the previous release are kept.
- Before it replaces anything, an install asks the new binary whether it has a JavaScript
  interpreter (`rusty-impulse --version`) and whether it runs the tree (`rusty-impulse --check-tree`). If
  it has one, or does not run the tree, nothing is installed. This holds for `BIN=` and
  `NOBUILD=1` too.
- With the config in place, an install or a rollback asks the binary the way Hyprland starts
  it, `rusty-impulse -c rusty-impulse --check-tree`. `rusty-impulse` finds the config named rusty-impulse as Quickshell does: in
  `~/.config/rustshell/rusty-impulse`, unless `~/.config/rustshell` has a `shell.qml` of its own. That
  file is then the default config and no named config beside it is looked up: the install or
  rollback is undone and says so. Move the file, with the files it uses, to
  `~/.config/rustshell/default/`: it stays the default config, and the original is found.
- A relative `XDG_CONFIG_HOME` or `XDG_STATE_HOME` is ignored, by `rusty-impulse` and by the script
  (XDG Base Directory Specification): the defaults under `$HOME` are used.
- If `$PREFIX/bin/rusty-impulse` runs a binary that no install put there (a file copied there, a link
  to a build elsewhere), that binary is first kept as a release, `legacy-<id>`, with the
  config it ran: `--rollback` goes back to it, and it still runs if the install fails. A
  link in that config to a file or a directory of the config itself (the original's tree has four
  icons that are links to other icons) is kept and comes back as that link; a link that
  leads out of the config, or names an absolute path, comes back as the file it led to.
- Installing the same binary again with another QML tree replaces the current release; the
  previous one stays the release to roll back to.
- `--rollback` switches to the previous release. If the two releases' trees differ, or that
  binary does not run the rusty-impulse config as it is now, the release's tree is put in the config's
  place; a config edited since its install is kept as for an install. A second `--rollback`
  goes forward again.
- An install or a rollback that fails at a step (the config or a link cannot be replaced)
  puts back what it has replaced, and an install removes the release it made: the release
  that was installed stays the current one, with the config it runs and the release to roll
  back to.
- An install or a rollback that gets SIGINT (Ctrl-C), SIGTERM or SIGHUP (its terminal was
  closed) undoes what it has switched and exits: the release that was installed stays, with
  the config it runs. One that is killed between its steps (SIGKILL, the power) leaves a
  shell that starts: until the config and the links are both switched,
  `$PREFIX/lib/rusty-impulse/current` names `releases/.switch`, whose `rusty-impulse` starts the release that
  runs the config as it is. `--status` says that an install was interrupted. The next install,
  rollback or uninstall first finishes the killed one if the config is already the new
  release's, and undoes it if not; it says which, and where a config that was replaced is
  kept.
- `--uninstall` removes the releases and the link `$PREFIX/bin/rusty-impulse`. The rusty-impulse config, the kept
  configs and an `rusty-impulse` of another origin under that name stay.
- The build is told what the tree holds (a checksum, `RI_ROOT_SUM`), so a tree whose files are
  older than the last build (after a rollback, a backup put back) is compiled too, and new
  file times or a new `.git` alone do not compile it again. Cargo alone goes by file times.
- Neither restarts a running shell. A reload (`Rustshell.reload()`) and the shell's own
  windows run `$PREFIX/bin/rusty-impulse`, so after an install they use the new release. A reload
  asked for while the session is locked waits for the unlock:
  [docs/quickshell-differences.md](quickshell-differences.md).
- Hyprland starts the shell as `rusty-impulse -c rusty-impulse`.
- `rusty-impulse ipc call <target> <function>` talks to the running shell, like `qs -c ii ipc call`.
- If the QML tree differs from the one compiled in, `rusty-impulse` prints the differing files and
  exits with status 1. Run `tools/install.sh` again.
