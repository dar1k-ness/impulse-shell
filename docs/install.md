# Install

## By one command

On Arch Linux with end-4's dots-hyprland in place:

```sh
git clone https://github.com/dar1k-ness/impulse-shell.git && impulse-shell/install.sh
```

`install.sh` goes through the steps below by itself and says each one before it makes it:

1. The packages the build needs and pacman does not have yet (`sudo pacman -S`, the list of
   [build.md](build.md) with `libheif`, `libavif` and `libthai`). The Rust toolchain comes from
   `rustup`; an installed package `rust` is replaced by `rustup`, which provides it.
2. `PREFIX=~/.local tools/install.sh`: build and install, as described under
   [The steps one by one](#the-steps-one-by-one).
3. `tools/carry-over.sh`, then `tools/carry-over.sh --dots new`: the settings come over from
   the original and the Hyprland files call and start `impulse-shell` (the header of `tools/carry-over.sh` says what exactly).
4. Inside a Hyprland session: Hyprland reloads its config, the shell that runs is stopped and
   `impulse-shell -c impulse-shell` is started. If its panels are not on screen in 30 seconds
   (`RI_START_WAIT`), the files of step 3 are put back and the shell that ran is started
   again; the script then ends with status 2. Outside a session nothing is started: the next
   login starts the shell.

- `install.sh --no-start` leaves the running shell alone; `install.sh --no-packages` installs
  no packages.
- The file alone, run from outside a checkout (`bash install.sh`), clones the repository to
  `~/.local/src/impulse-shell` (`RI_SRC`; `RI_REPO` names another repository) or updates the
  clone that is there, and installs that.
- Running it again updates: it builds what the checkout has now and installs it as a new
  release. A shell that already runs the installed build is left running.
- Back to the original: `tools/carry-over.sh --dots old`, then log in again.
  `tools/install.sh --uninstall` removes the build.

## The steps one by one

```sh
tools/install.sh              # build, then install to /usr/local/bin/impulse-shell
PREFIX=~/.local tools/install.sh
NOBUILD=1 tools/install.sh    # install the existing build (the one in Cargo's target directory)
BIN=<file> tools/install.sh   # install an impulse-shell built elsewhere
tools/install.sh --rollback   # back to the release the last install replaced
tools/install.sh --status     # the current and the previous release, and the impulse-shell config
tools/install.sh --mark-good  # the current release has proved good: kept until another one is marked
tools/install.sh --rollback-good  # back to the release marked good
tools/install.sh --uninstall
```

- It compiles this checkout's `qml` (`RI_ROOT` overrides) and makes the impulse-shell config
  `~/.config/rustshell/impulse-shell`, what `impulse-shell -c impulse-shell` runs, a copy of it, with the files'
  executable bits. A config that was there with other content is kept as
  `~/.local/state/impulse-shell/replaced-<time>.<random>`, a link in it as the link; the script never
  removes these copies. Run it as the user whose shell it is, not as root (`sudo`, `doas`,
  `pkexec`, `su`).
- It uses `sudo` only when the target directories are not writable.
- The binary and a copy of the QML tree it was compiled from are kept together as a release in
  `$PREFIX/lib/impulse-shell/releases/<id>`; `$PREFIX/bin/impulse-shell` is a link to the current one. The
  current and the previous release are kept.
- Before it replaces anything, an install asks the new binary whether it has a JavaScript
  interpreter (`impulse-shell --version`) and whether it runs the tree (`impulse-shell --check-tree`). If
  it has one, or does not run the tree, nothing is installed. This holds for `BIN=` and
  `NOBUILD=1` too.
- With the config in place, an install or a rollback asks the binary the way Hyprland starts
  it, `impulse-shell -c impulse-shell --check-tree`. `impulse-shell` finds the config named impulse-shell as Quickshell does: in
  `~/.config/rustshell/impulse-shell`, unless `~/.config/rustshell` has a `shell.qml` of its own. That
  file is then the default config and no named config beside it is looked up: the install or
  rollback is undone and says so. Move the file, with the files it uses, to
  `~/.config/rustshell/default/`: it stays the default config, and the original is found.
- A relative `XDG_CONFIG_HOME` or `XDG_STATE_HOME` is ignored, by `impulse-shell` and by the script
  (XDG Base Directory Specification): the defaults under `$HOME` are used.
- If `$PREFIX/bin/impulse-shell` runs a binary that no install put there (a file copied there, a link
  to a build elsewhere), that binary is first kept as a release, `legacy-<id>`, with the
  config it ran: `--rollback` goes back to it, and it still runs if the install fails. A
  link in that config to a file or a directory of the config itself (the original's tree has four
  icons that are links to other icons) is kept and comes back as that link; a link that
  leads out of the config, or names an absolute path, comes back as the file it led to.
- Installing the same binary again with another QML tree replaces the current release; the
  previous one stays the release to roll back to.
- Two installs in a row leave the last two releases: `--rollback` then goes to the one the
  first of them made, and the release that ran before both is gone. A release that has
  proved good is therefore marked (`--mark-good`; `SHA256=<hex>` marks it only if that is its
  binary): it is kept beside the current and the previous one until another release is
  marked, and `--rollback-good` switches to it the way `--rollback` switches to the previous
  one. The release switched from becomes the previous one. `tools/pc/rollback-watch` marks
  the release that runs once its shell has run half an hour, and goes back to the marked
  release first when a later one goes away.
- `--rollback` switches to the previous release. If the two releases' trees differ, or that
  binary does not run the impulse-shell config as it is now, the release's tree is put in the config's
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
  `$PREFIX/lib/impulse-shell/current` names `releases/.switch`, whose `impulse-shell` starts the release that
  runs the config as it is. `--status` says that an install was interrupted. The next install,
  rollback or uninstall first finishes the killed one if the config is already the new
  release's, and undoes it if not; it says which, and where a config that was replaced is
  kept.
- `--uninstall` removes the releases and the link `$PREFIX/bin/impulse-shell`. The impulse-shell config, the kept
  configs and an `impulse-shell` of another origin under that name stay.
- The build is told what the tree holds (a checksum, `RI_ROOT_SUM`), so a tree whose files are
  older than the last build (after a rollback, a backup put back) is compiled too, and new
  file times or a new `.git` alone do not compile it again. Cargo alone goes by file times.
- Neither restarts a running shell. A reload (`Rustshell.reload()`) and the shell's own
  windows run `$PREFIX/bin/impulse-shell`, so after an install they use the new release. A reload
  asked for while the session is locked waits for the unlock:
  [docs/quickshell-differences.md](quickshell-differences.md).
- Hyprland starts the shell as `impulse-shell -c impulse-shell`.
- `impulse-shell ipc call <target> <function>` talks to the running shell, like `qs -c ii ipc call`.
- If the QML tree differs from the one compiled in, `impulse-shell` prints the differing files and
  exits with status 1. Run `tools/install.sh` again.
