# kmyc-golden: fixtures from the original kde-material-you-colors

Golden outputs for the native Rust port of `kde-material-you-colors` (equivalence tests in
`crates/colorgen/tests/kde/`). Each case is ONE apply of the original, run the way the original's
wrapper runs it: `kde-material-you-colors -d|-l --color '#RRGGBB' -sv N`.

## Rebuild

```sh
python3 tools/reference/kmyc-golden/mkcases.py   # only if the case list changed (rewrites tests/kde/cases/)
tools/reference/kmyc-golden/gen.sh               # all cases (~40 s); or: gen.sh <case>...
```

`gen.sh` builds the `kmyc-golden` image from `Containerfile` (docker or podman, `ENGINE=`
to pick) and re-runs itself inside it with the repo mounted at `/src`.

Per case (`tests/kde/cases/<case>/`): `args` (one argv per line), optional `config.conf`
(copied to `~/.config/kde-material-you-colors/config.conf`), optional `home/` (pre-existing
files copied into HOME). Each run gets a fresh `HOME=$(mktemp -d)/home` and `TMPDIR`, a clean
env (`env -i`, no `XDG_*`, `USER=kmyc`, `LANG=C.UTF-8`, `QT_QPA_PLATFORM=offscreen`,
`OPENBLAS_NUM_THREADS=1`) and its own `dbus-run-session` with `dbus-monitor --session`
recording. `once.py` patches `apply_themes.apply` to stop after the first apply (no scheme
cache, unlike the original's `kde-material-you-colors-once.py`).

Output (`tests/kde/expected/<case>/`):
- every file under HOME the run created or changed, same relative path (mode normalised to 644);
  the program's own log `~/.local/share/kde-material-you-colors/*.log` is skipped (timestamps);
- `export.json`: `$TMPDIR/kde-material-you-colors-$USER.json`;
- `dbus.log`: see below;
- `outcome.txt`: `ok`, or the exception that escaped `apply()`;
- `deleted.txt` if the run deleted files (no current case does);
- `shared.txt`: see dedup below.

`KEEP_WORK=1 gen.sh <case>` keeps the work dir (raw `dbus.raw`, `stdout` with the program log).

### Dedup (`shared.txt`)

Most files are the same across cases (a light and a dark run of one seed write the same
`.colors`/export, config cases share the seed's files...). `dedup.py` keeps one copy: a line
`<relpath>\t<owner>` in `<case>/shared.txt` means the file is byte-identical to
`expected/<owner>/<relpath>` (owners are never chained; seed-*, sv*, defaults-* are preferred
owners). A test must resolve these, e.g. `dedup.py expand DIR` on a copy. Expanded size is
~7.7 MB, stored ~3.3 MB (export.json is ~100 KB per distinct scheme).

## Versions

Container: `archlinux:latest` on 2026-11-04.

```
plasma-workspace 6.8.2-1   kconfig 6.31.0-1   qt6-base 6.12.0-3   breeze-icons 6.31.0-1
python 3.14.9-1            python-dbus 1.5.1-1   dbus 1.16.2-2
```

venv (`--system-site-packages`, `pip freeze`, system packages included):
```
dbus-python==1.5.1
kde-material-you-colors==1.10.1
materialyoucolor==2.0.10   (no cp314 wheel: built from source with gcc, as on a desktop running Python 3.14)
numpy==2.5.4
pillow==13.0.0
pywal==3.3.0               (the `pywal-*` cases; no xrdb/i3/sway/kitty/polybar in the image)
```

`breeze-plus` / `breeze-plus-dark` (icon themes that are AUR-only) are faked in the
image as `/usr/share/icons/<name>/index.theme` (`Inherits=breeze`).

## Accent colour (`kde/accent.rs`)

- `accent-*` cases: kdeglobals with `[General] AccentColor`, run through the original like
  the others (`plasma-apply-colorscheme` then takes Plasma's accent path of `applyScheme`).
- `plasma.sh` regenerates `tests/kde/plasma/expected/`: the real `plasma-apply-colorscheme`
  alone on `tests/kde/plasma/cases/*` (`mkcases.py` writes them), scheme settings the
  original never writes (`TintFactor`, `TitlebarIsAccentColored`, `accentActiveTitlebar`) and
  Breeze schemes. It runs as root in the image: as an unknown uid `plasma-apply-colorscheme`
  cannot open the scheme and silently applies garbage.
- `qcolor.sh` regenerates `tests/kde/qcolor.txt` (`qcolor.cpp`: Plasma's colour helpers
  verbatim on the real Qt + KGuiAddons, 4000 cases) for `tests/kde_qcolor.rs`.
- `accent_vectors.sh` regenerates `tests/kde/accent_vectors.txt` for the unit tests in
  `kde/accent.rs`: named vectors from `accent_vectors.cpp` (Qt `QColor`/`QColorTransform`,
  KConfig colour entries, `KColorUtils`, Plasma's helpers verbatim) and the `[Colors:*]`/`[WM]`
  entries the real `plasma-apply-colorscheme` writes for light and dark Material You schemes
  (seed-5e7a91) with an `AccentColor`.

## What the original does here (worth knowing for the port)

- Every case ends with `outcome.txt` = `DBusException ...ServiceUnknown`: the default
  `titlebar_opacity` is clipped to 100 (never None), so every apply touches Klassy/SBE
  configs, sends the Klassy signal and finally calls `org.kde.KWin.reconfigure`, which
  fails with no KWin on the bus (the same on a Hyprland desktop). Steps after it
  (pywal palette print, `on_change_hook`) never run; the pywal export (`pywal = True`) runs
  before it, so `pywal-*` cases hold `~/.cache/wal` (incl. `sequences`, what every terminal got).
- `pywal_light` (unless `pywal_follows_scheme`) also picks the Konsole scheme and opacity mode,
  with or without `pywal` (case `cfg-pywal-light-only`).
- `plasma-apply-colorscheme` runs twice (the `...2.colors` copy first, then the real one);
  each run tries `org.kde.KWin.BlendChanges.start` and emits `kconfig.notify ConfigChanged`
  + `KGlobalSettings.notifyChange`. Then the python side rewrites kdeglobals with
  configparser (adds `ColorSchemeHash` = sha1 of the applied `.colors`, drops comments).
- `plasma-changeicons` with a theme that has no directory anywhere
  (case `cfg-icons-missing`): it does not check, exits 0, writes `[Icons] Theme=no-such-theme`
  to kdeglobals and emits the same six `KIconLoader.iconChanged` signals.
- Klassy titlebar opacity reuses the ConfigParser that already read `sierrabreezeenhancedrc`,
  so `klassyrc` gets SBE's sections merged in (case `pre-klassy-sbe`). A klassyrc without
  `[TitleBarOpacity]` is left untouched (KeyError swallowed).
- `konsolerc`: no `DefaultProfile` → inserted after `[Desktop Entry]` (or appended with a
  new section); `DefaultProfile=` empty → filled; a set profile is edited in place.

## Nondeterminism normalised

- `dbus.log`: from the raw monitor output only signals, method calls and errors are kept,
  without time/serial/sender; unique names become `:X`; the bus driver's plumbing (Hello,
  AddMatch, RemoveMatch, NameOwnerChanged/NameAcquired...) is dropped, but ListNames /
  GetNameOwner / StartServiceByName / NameHasOwner stay. `kconfig.notify ConfigChanged`
  carries an `a{saay}` built from a QHash (random order per process): entries of every
  array of dict entries are sorted.
- HOME/TMPDIR are random but no output contains them.
- With that, two full runs were byte-identical.
