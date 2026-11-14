Files of ii that the shell does not use, kept where they were under `qml` (same relative
paths) because tests still read them. Nothing here is installed: `tools/install.sh` copies
`qml` only.

- `scripts/`, `services/gCloud/`, `translations/tools/`: ii's shell and Python scripts. The
  shell does their work in-process; the equivalence tests run them as the reference side.
  `scripts/colors/terminal/` (templates the colour generation reads) is still in `qml/scripts`.
- `modules/`: QML that no window of the shell loads any more.
  `aiChat/AttachedFileIndicator.qml` holds the blocks that the test of `FileMimeType` and
  `ImageSize` (types the shell still uses) runs as ii wrote them; `StyledSpinBox.qml` is the
  spin box of the `interact` harness.
