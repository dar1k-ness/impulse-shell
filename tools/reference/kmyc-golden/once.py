"""Run the original kde-material-you-colors main() until its first apply, then exit.

argv: kde-material-you-colors CLI arguments. Writes $KMYC_OUTCOME with "ok" or the
exception that escaped apply() (the original keeps going; on a Hyprland desktop the
final KWin reload has no KWin to talk to, so the apply ends in a DBusException there too).
"""
import os
import sys

from kde_material_you_colors import apply_themes


class _Applied(Exception):
    pass


_apply = apply_themes.apply


def _apply_once(*args, **kwargs):
    outcome = "ok"
    try:
        _apply(*args, **kwargs)
    except Exception as e:  # noqa: BLE001
        name = getattr(e, "get_dbus_name", lambda: None)()
        outcome = f"{type(e).__module__}.{type(e).__name__}" + (f" {name}" if name else "")
    with open(os.environ["KMYC_OUTCOME"], "w", encoding="utf-8") as f:
        f.write(outcome + "\n")
    raise _Applied


apply_themes.apply = _apply_once
sys.argv = ["kde-material-you-colors", *sys.argv[1:]]
try:
    from kde_material_you_colors import main  # noqa: F401  (importing runs main())
except _Applied:
    pass
sys.exit(0)
