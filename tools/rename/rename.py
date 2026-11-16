#!/usr/bin/env python3
"""Rename the project's old names (illogical-impulse, ii) to rusty-impulse.

Pass 1 (`--pass 1`, the default) renames what the project calls itself: the binary, the crates,
the QML module directory, the config tree, the `II_*` variables and the test names built from
them. Pass 2 renames the engine the shell implements (`quickshell` in our own paths, layer
namespaces, D-Bus names and the QML module) to the name in ENGINE.

Pass 4 renames the project a second time, rusty-impulse to impulse-shell (NEW): the program,
its config and settings directories, the release directory, the variables and test names made
from the name. What the screen shows becomes the short name (SHOWN). The engine keeps its names
(`rustshell`, the `Rustshell` module, `ri.`, `RI_*`, the `rusty` panel family and module
directory). It reads the files passes 1 to 3 leave alone too, the carry-over and the PC scripts:
in them it is the new build that gets the new name.

Neither pass touches what belongs to someone else: the files under SKIP, the lines that credit
end-4, or a word that only looks like a name (`ASCII`, the `ii_CN` locale, a `(ii)` D-Bus
signature). Both are idempotent: running one twice changes nothing the second time.

    tools/rename/rename.py --pass 1            # rename, in the working tree
    tools/rename/rename.py --pass 1 --check    # what it would change, changing nothing
    tools/rename/apply4.sh [PICK...]           # pass 4 as a commit on the checked-out head
"""

import argparse
import os
import re
import subprocess
import sys

import prose

ENGINE = "rustshell"  # what the engine is called instead of quickshell; one word, no dash

NEW = "impulse-shell"  # pass 4: the program, its directories, the repository
SHOWN = "Impulse"  # pass 4: what the screen shows

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# Files this never reads: someone else's code and data. A name of ours inside them is theirs, not
# a leftover. The copies of the original's scripts under tests/fixtures are renamed like the tree:
# a test runs one beside its native port in one home, so both have to look in the same places.
SKIP = (
    "vendor/",
    "crates/qml/src/syntax/data/",  # the editor's syntax definitions (IIOImage, MIPS_II)
    "crates/qml/src/locale_data.rs",  # the "ii" locale (Sichuan Yi) and its names
    "crates/qml/src/locale_num.rs",  # IiNn and the other number formats
    "crates/qsg/src/text.rs",  # the Yiii script
    "crates/qml/tests/locale_case_qt/",  # Qt's case tables (IISS)
    "Cargo.lock",  # written by cargo after the crates are renamed
    "tools/rename/",
    # the carry-over of the user's data and the PC's launchers: they name the old places on purpose
    "tools/carry-over.sh",
    "tools/pc/",
    "crates/shell/tests/carry_over_dots.rs",  # the old commands the carry-over rewrites
    "crates/shell/tests/pc_switch_keyring",  # the old build's keyring item and launchers, as switch finds them
    "crates/shell/tests/pc_rollback_watch",  # the old build the watch puts back, as switch finds it
    # a window the comparison gives to both shells, in the original's words (scenes.sh)
    "tools/parity/probe_qtr.qml",
)
# Pass 2 also leaves what is compared with the real Quickshell, in its own words: the tests that
# start it and the QML they give to both shells, the scripts that measure it, the page about it.
SKIP2 = SKIP + (
    "crates/qml/tests/appsearch/",
    "crates/qml/tests/desktopwatch/",
    "crates/qml/tests/fswatch/",
    "crates/qml/tests/icon_image_size/",
    "crates/qml/tests/openurl_relative/",
    "crates/qml/tests/qs_base_url/",
    "crates/qml/tests/unwritten/",
    "crates/shell/tests/code_colours/",
    "crates/shell/tests/destruction/",
    "crates/shell/tests/image_status/",
    "crates/shell/tests/interact/",
    "crates/shell/tests/lazy_loader/",
    "crates/shell/tests/notification_clear_all/",
    "crates/shell/tests/plain_layer_qt/",
    "crates/shell/tests/qs_base_url/",
    "crates/shell/tests/qs_urls/",
    "crates/shell/tests/reload/",
    "crates/shell/tests/scenes/script_model_current/",
    "crates/shell/tests/session_lock_reload/",
    "crates/shell/tests/sync_load/",
    "crates/qml/tests/appsearch.rs",
    "crates/qml/tests/desktopwatch.rs",
    "crates/qml/tests/fswatch.rs",
    "crates/qml/tests/icon_image_size.rs",
    "crates/qml/tests/openurl_relative.rs",
    "crates/qml/tests/probe_equiv.rs",
    "crates/qml/tests/qs_base_url.rs",
    "crates/qml/tests/script_model_rows.rs",
    "crates/qml/tests/unwritten_methods.rs",
    "crates/qml/tests/variants_values.rs",
    "crates/shell/tests/anim_first_step.rs",
    "crates/shell/tests/behavior_before_show_equiv.rs",
    "crates/shell/tests/child_env_equiv.rs",
    "crates/shell/tests/code_colours.rs",
    "crates/shell/tests/config_lookup_equiv.rs",
    "crates/shell/tests/destruction_equiv.rs",
    "crates/shell/tests/frame_animation_equiv.rs",
    "crates/shell/tests/icon_request_scale_equiv.rs",
    "crates/shell/tests/icon_theme_dirs.rs",
    "crates/shell/tests/image_status_qt.rs",
    "crates/shell/tests/interact.rs",
    "crates/shell/tests/ipc_duplicate_target_equiv.rs",
    "crates/shell/tests/ipc_equiv.rs",
    "crates/shell/tests/key_before_activation_equiv.rs",
    "crates/shell/tests/keyboard_enter_equiv.rs",
    "crates/shell/tests/lazy_loader_equiv.rs",
    "crates/shell/tests/notification_clear_all_equiv.rs",
    "crates/shell/tests/plain_layer_qt.rs",
    "crates/shell/tests/posted_chain_frames_equiv.rs",
    "crates/shell/tests/qs_list_log.rs",
    "crates/shell/tests/qs_urls.rs",
    "crates/shell/tests/reload_equiv.rs",
    "crates/shell/tests/scale_factor_equiv.rs",
    "crates/shell/tests/script_model_current.rs",
    "crates/shell/tests/session_lock_reload.rs",
    "crates/shell/tests/support/qt_window.rs",
    "crates/shell/tests/swipe_start_equiv.rs",
    "crates/shell/tests/sync_load.rs",
    "crates/shell/tests/timer_clock_equiv.rs",
    "crates/shell/tests/timer_steps_equiv.rs",
    "crates/shell/tests/tray_pixels_qt.rs",
    "crates/shell/tests/wallpaper_change_equiv.rs",
    "crates/shell/tests/wallpaper_scale_equiv.rs",
    "crates/shell/tests/windows_outputs.rs",
    "docs/quickshell-differences.md",
    "crates/testenv/src/impulse_qt.rs",  # writes the Impulse module real Quickshell is given
    "tools/parity/",
    "bench/",
)
# The same for a path: these files stay where they are.
KEEP_PATH = (
    "vendor/",
    "tools/rename/",
    "tools/carry-over.sh",
    "tools/pc/",
)

# Pass 4 reads everything of ours, the carry-over and the PC scripts too.
SKIP4 = (
    "vendor/",
    "Cargo.lock",
    "tools/rename/",
)
KEEP_PATH4 = (
    "vendor/",
    "tools/rename/",
)

# A line that credits the original keeps its name (GPL).
CREDIT = re.compile(r"end-4|dots-hyprland")

# Hand edits the rules cannot express: (file after the rename, old text, new text). Each is
# applied once; one whose old text is gone and whose new text is not there stops the pass.
INSTALL = r"^(tools/install\.sh|crates/shell/tests/install_rollback\.rs)$"

NOT_RUST = r"(?<!\.rs)$"
# documents that name the command the original was run with, `qs -c ii`: it stays, Quickshell
# has no config called rusty-impulse
# the PC script that makes the native shell or the original the one Hyprland starts; it comes with
# the carry-over (apply.sh) and is not there before it
PC_DEFAULT = "tools/pc/rusty-impulse-default"
ORIGINAL_COMMAND = r"^(docs/install\.md|docs/benchmarks\.md|bench/qt-flood-profile\.md|tools/stress/REPORT\.md)$"

# Pass 3, after prose.py: the places it cannot tell from their words.
LEFT = [
    # what a script says: the config and the tree are this shell's
    (r"(?<![\w/.\-'`$=])ii configs?\b", lambda m: m.group(0).replace("ii", "rusty-impulse", 1), NOT_RUST),
    (r"(?<![\w/.\-'`$=])ii tree\b", "QML tree", NOT_RUST),
    (r"<ii tree>", "<QML tree>"),
    (r"\b([Oo]riginal) ii\b(?![-/'])", r"\1", NOT_RUST),
    (r"\b([Oo]riginal) ii's", r"\1's", NOT_RUST),
    (r"(?<![\w/.\-'`$=])ii's\b", "the original's", NOT_RUST),
    (r": ii picks one of two shapes", ": the original picks one of two shapes", r"^tools/parity/masks\.json$"),
    (r"\bwhole ii tree\b", "whole QML tree", NOT_RUST),
    (r"\bconfig `ii`", "config `rusty-impulse`", r"^bench/vm\.sh$"),
    (r"^ii/\*\.qml, \*\.js$", "qml/*.qml, *.js", r"^docs/aot\.md$"),
    (r"<ii's line>", "<the original's line>", r"^crates/qml/tests/probe_equiv\.rs$"),
    (r"the parent of `ii`", "the parent of the tree", r"^crates/qml/tests/ai_scripts\.rs$"),
    (r"Name=ii test", "Name=test", r"^crates/qml/tests/sound_qalc_gcloud_booru_pin_equiv\.rs$"),
    # names a script gives its own files
    (r'"\$b/ii"', '"$b/tree"', r"^tools/reference/kaggle-gpu/"),
    (r'os\.path\.join\(b, "ii"\)', 'os.path.join(b, "tree")', r"^tools/reference/kaggle-gpu/"),
    (r"\bORIG_II\b", "ORIG_TREE", r"^tools/stress/"),
    (r"~/ii\.log", "~/rusty-impulse.log", r"^tools/stress/"),
    (r"back_ii\.png", "back_rusty.png", r"^tools/stress/"),
    (r"relaunch ii$", "relaunch rusty", r"^tools/parity/scenes\.sh$"),
    (r"`shell-ii\.log`", "`shell-rusty.log`", r"^tools/parity/REPORT\.md$"),
    (r"/tmp/ii\(scen\|flood\)-\[0-9\]\+#/tmp/ii", "/tmp/ri(scen|flood)-[0-9]+#/tmp/ri", r"^tools/reference/equiv/debugenv\.sh$"),
    (r"\bsuch as ii\b", "such as rusty-impulse", INSTALL),
    (r"\(ii's tree has", "(the tree has", INSTALL),
]

EDITS = {
    3: [
        # the user's files that reach the shell through Quickshell's client (`qs -c ii ipc call ...`,
        # `qs -p <its tree>/settings.qml`, the fish alias): after the rename qs does not know the
        # shell, the command becomes the build's own binary; `qs -c ii` that starts the original stays
        ("tools/carry-over.sh", 'rewrite() {\n    sed -E \\\n', '#\n# The old build answered Quickshell\'s own client: "qs -c ii ipc call ..." reached it, and\n# "qs -p <its tree>/settings.qml" opened its windows. The new build has a socket and a tree of\n# its own that qs does not know, so where a line calls qs for this shell (its config by name or\n# by \\$qsConfig, or a file of its tree) the command becomes the new build\'s own binary: by its\n# full path under ~/.local/bin when it is installed there (that directory is not on Hyprland\'s\n# PATH), else by name. An alias for "qs -c ii" (fish: alias q \'qs -c ii\', used as "q ipc call\n# ...") becomes the new build\'s too. Any other "qs -c ii" starts a shell and stays as it is:\n# that is the original.\n#\n# "qs -c \\$qsConfig" without "ipc" starts the shell the variable names (end-4\'s execs.lua at\n# login, and what Ctrl+Super+R runs after its killall). The variable is rewritten to the new\n# build\'s tree, which qs cannot start, so that command becomes the new build\'s binary too. A\n# killall or pidof that lists "qs quickshell" (the restart binding, hypridle\'s "is a shell\n# there to lock the screen") gets the new build\'s name after them: its process is neither.\nrewrite() {\n    local client=$NEW_BIN ours="($OLD_TREE|\\\\\\$\\\\{?qsConfig\\\\}?)"\n    [ ! -x "$HOME/.local/bin/$NEW_BIN" ] || client="\\$HOME/.local/bin/$NEW_BIN"\n    sed -E \\\n        -e "s#(^|[^[:alnum:]_./-])qs( +-c +[\\"\']?$ours[\\"\']? +ipc([^[:alnum:]_-]|\\$))#\\\\1$client\\\\2#g" \\\n        -e "s#(^|[^[:alnum:]_./-])qs( +(-p|--path) +[^ \\"\']*$OLD_WORD/$ours/)#\\\\1$client\\\\2#g" \\\n        -e "s#(^|[^[:alnum:]_./-])qs( +-c +[\\"\']?\\\\\\$\\\\{?qsConfig\\\\}?[\\"\']?([^[:alnum:]_\\"\'-]|\\$))#\\\\1$client\\\\2#g" \\\n        -e "s#((^|[^[:alnum:]_./-])(pidof|killall)( +[^;&|]*)? +qs +quickshell)( +$NEW_BIN)?([^[:alnum:]_.-]|\\$)#\\\\1 $NEW_BIN\\\\6#g" \\\n        -e "/(^|[^[:alnum:]_-])alias[ =]/s#(^|[^[:alnum:]_./-])qs( +-c +[\\"\']?$OLD_TREE[\\"\']?([^[:alnum:]_-]|\\$))#\\\\1$client\\\\2#g" \\\n        -e "s#(^|[^[:alnum:]_./-])(qs +-c +[\\"\']?)$OLD_TREE([\\"\']?)([^[:alnum:]_\\"\'-]|\\$)#\\\\1\\\\2\\\\x01\\\\3\\\\4#g" \\\n'),
        ("tools/carry-over.sh", '        -e "s#(-c[ \\"\',]+|qsConfig[\\"\']?[ ]*[=,][ ]*[\\"\'])$OLD_TREE([^[:alnum:]_-]|\\$)#\\\\1$NEW_TREE\\\\2#g"\n}\n', '        -e "s#(-c[ \\"\',]+|qsConfig[\\"\']?[ ]*[=,][ ]*[\\"\'])$OLD_TREE([^[:alnum:]_-]|\\$)#\\\\1$NEW_TREE\\\\2#g" \\\n        -e "s#\\\\x01#$OLD_TREE#g"\n}\n'),
        # install.sh --rollback with no earlier renamed build: the rollback-now that works is the
        # one switch filled in beside the rollback copies, not the one of a checkout
        (
            "tools/install.sh",
            '    echo "; the build from before the rename is still installed under its old names ($bin_dir/$bin, $config_home/$word/$tree): tools/pc/rollback-now starts it again, this script does not switch to it"\n',
            '    echo "; the build from before the rename is still installed under its old names ($bin_dir/$bin, $config_home/$word/$tree): this script does not switch to it. What starts it again is the rollback-now that tools/pc/switch filled in, in the build directory switch was run from (~/pcbuild/stable-<sha>/rollback-now); tools/pc/rollback-now of a checkout has no rollback copies and does nothing"\n',
        ),
        # the PC's switch between the native shell and the original: the original is still
        # `qs -c ii` in ~/.config/quickshell/ii with quickshell:* layers, and the rollback watch that
        # ended when it saw Quickshell is started again once the shell is the native one
        (
            PC_DEFAULT,
            (
                '# rusty-impulse-default native|qt: make native rusty-impulse or Qt `qs -c rusty-impulse` the shell Hyprland starts at login\n'
            ),
            (
                '# rusty-impulse-default native|qt: make native rusty-impulse or Qt `qs -c ii` (the original, drawn by\n'
                '# Quickshell; `qs -c rusty-impulse` where its tree is under that name) the shell Hyprland starts at login\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                '# qs -c rusty-impulse reads its own directory, not the tree install.sh puts for the native shell,\n'
                '# and it has neither the Impulse nor the Rustshell module: a tree that imports them ends at once\n'
                '# with \'module "Impulse" is not installed\'. Look before the native shell is stopped.\n'
                'QS_TREE=${XDG_CONFIG_HOME:-$HOME/.config}/quickshell/rusty-impulse\n'
            ),
            (
                '# The Qt shell is the original, which Quickshell reads from ~/.config/quickshell/NAME as qs -c NAME,\n'
                '# not the tree install.sh puts for the native shell (~/.config/rustshell/rusty-impulse). NAME is\n'
                '# ii, the name it always had and the one a PC switched from the old build keeps; where there is\n'
                '# no such tree, one put there under the new name is taken. Quickshell has neither the Impulse nor\n'
                '# the Rustshell module: a tree that imports them ends at once with \'module "Impulse" is not\n'
                "# installed'. Look before the native shell is stopped.\n"
                'QS_NAME=\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                '  [ -f "$QS_TREE/shell.qml" ] || {\n'
                '    echo "rusty-impulse-default: qs -c rusty-impulse reads $QS_TREE and there is no shell.qml there (the installed tree is the native shell\'s); nothing changed" >&2; exit 1; }\n'
                '  f=$(grep -rlE --include=\'*.qml\' \'^import (Impulse|Rustshell)([ .]|$)\' "$QS_TREE/" 2>/dev/null | head -1)\n'
                '  [ -z "$f" ] || {\n'
                '    echo "rusty-impulse-default: the tree in $QS_TREE imports modules only the native shell has (first file: $f), Quickshell cannot draw it; nothing changed" >&2; exit 1; }\n'
            ),
            (
                '  why=\n'
                '  for n in ii rusty-impulse; do\n'
                '    d=${XDG_CONFIG_HOME:-$HOME/.config}/quickshell/$n\n'
                '    [ -f "$d/shell.qml" ] || { why+=" no shell.qml in $d;"; continue; }\n'
                '    f=$(grep -rlE --include=\'*.qml\' \'^import (Impulse|Rustshell)([ .]|$)\' "$d/" 2>/dev/null | head -1)\n'
                '    [ -z "$f" ] || { why+=" the tree in $d imports modules only the native shell has (first file: $f);"; continue; }\n'
                '    QS_NAME=$n; break\n'
                '  done\n'
                '  [ -n "$QS_NAME" ] || { echo "rusty-impulse-default: there is no tree Quickshell can draw:$why nothing changed" >&2; exit 1; }\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                "QT_START='qs -c $qsConfig'; NAT_START='$HOME/.local/bin/rusty-impulse-shell'\n"
                '# setstart FROM TO: the shell start line of the autostart and of the Ctrl+Super+R bind.\n'
            ),
            (
                'QT_START="qs -c $QS_NAME"; NAT_START=\'$HOME/.local/bin/rusty-impulse-shell\'\n'
                '# A Qt start line whatever config it names: qs -c $qsConfig is what this script wrote before, and\n'
                "# $qsConfig is the native shell's config since the rename.\n"
                "QT_ANY='qs -c [A-Za-z0-9_$-]*'\n"
                '# setstart TO: the shell start line of the autostart and of the Ctrl+Super+R bind.\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                '  sed -i "s|hl.exec_cmd(\\"$1\\")|hl.exec_cmd(\\"$2\\")|" $H/execs.lua\n'
                '  sed -i "s|killall ydotool qs quickshell rusty-impulse; $1|killall ydotool qs quickshell rusty-impulse; $2|" $H/keybinds.lua\n'
            ),
            (
                '  local was\n'
                '  for was in "$QT_ANY" "$NAT_START"; do\n'
                '    sed -i "s|hl.exec_cmd(\\"$was\\")|hl.exec_cmd(\\"$1\\")|" $H/execs.lua\n'
                '    sed -i "s|killall ydotool qs quickshell rusty-impulse; $was|killall ydotool qs quickshell rusty-impulse; $1|" $H/keybinds.lua\n'
                '  done\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                'if [ $mode = native ]; then from=$QT_START; to=$NAT_START; else from=$NAT_START; to=$QT_START; fi\n'
            ),
            (
                'if [ $mode = native ]; then to=$NAT_START; else to=$QT_START; fi\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                'setstart "$from" "$to"\n'
            ),
            (
                'setstart "$to"\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                '# Is the shell answering? The native shell is asked with its own client (no qs needed).\n'
                'ipc_ok() { local c=$HOME/.local/bin/rusty-impulse; [ $mode = qt ] && c=qs; timeout 3 $c -c rusty-impulse ipc show >/dev/null 2>&1; }\n'
            ),
            (
                '# Is the shell answering? Each is asked with its own client and by its own name: qs -c NAME the\n'
                '# original, rusty-impulse -c rusty-impulse the native shell (no qs needed). So with its bar: the\n'
                "# original's layers are quickshell:*, the native shell's rustshell:*.\n"
                'ipc_ok() {\n'
                '  if [ $mode = qt ]; then timeout 3 $QT_START ipc show >/dev/null 2>&1\n'
                '  else timeout 3 $HOME/.local/bin/rusty-impulse -c rusty-impulse ipc show >/dev/null 2>&1; fi\n'
                '}\n'
                'bar_up() { local ns=rustshell; [ $mode = qt ] && ns=quickshell; hyprctl layers | grep -q "$ns:bar"; }\n'
                '# The rollback watch (rollback-watch) ends by itself once it sees the Qt shell up. Where it runs\n'
                '# now, or is set to start at login, it is started again when the shell is the native one again:\n'
                '# a switch to Qt that failed must not leave the native shell without it. A watch started by hand\n'
                '# (rollback-watch start, no unit) is found again after a switch to Qt that worked by the path\n'
                '# written down then, until the next login.\n'
                'WUNIT=rusty-impulse-rollback-watch.service WSAVED=$XDG_RUNTIME_DIR/rusty-impulse-default.watch\n'
                'watch_path() {\n'
                '  local p; p=$(cat $XDG_RUNTIME_DIR/rusty-impulse-rollback-watch.pid 2>/dev/null) || return 0\n'
                '  [ -n "$p" ] && tr \'\\0\' \'\\n\' < /proc/$p/cmdline 2>/dev/null | grep -m1 \'/rollback-watch$\'\n'
                '}\n'
                'watch=$(watch_path); [ -n "$watch" ] || { watch=$(cat $WSAVED 2>/dev/null); [ -x "$watch" ] || watch=; }\n'
                'watch_again() {\n'
                '  [ -n "$watch" ] || systemctl --user -q is-enabled $WUNIT 2>/dev/null || return 0\n'
                '  sleep 2 # a watch that saw the Qt shell is on its way out\n'
                '  [ -z "$(watch_path)" ] || return 0\n'
                '  if systemctl --user -q is-enabled $WUNIT 2>/dev/null; then systemctl --user start $WUNIT; sleep 2\n'
                '  else "$watch" start >/dev/null 2>&1; fi\n'
                '  if [ -n "$(watch_path)" ]; then echo "rollback watch: started again"; rm -f $WSAVED\n'
                '  else echo "ERROR: the rollback watch is not running and did not start again: ${watch:-systemctl --user start $WUNIT}"; fi\n'
                '}\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                '    # binary under another name (ii-childguard) is not a second shell.\n'
            ),
            (
                '    # binary under another name (ri-childguard) is not a second shell.\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                '  else hyprctl dispatch \'hl.dsp.exec_cmd("qs -c rusty-impulse")\' >/dev/null; fi\n'
                '  local up=""\n'
            ),
            (
                '  else hyprctl dispatch "hl.dsp.exec_cmd(\\"$QT_START\\")" >/dev/null; fi\n'
                '  # 20 s, less than the 30 s (QT_WAIT) rollback-watch gives a Qt shell before it puts the\n'
                '  # previous build back: a Qt shell that draws nothing must be given up here first.\n'
                '  local up="" t0=$SECONDS\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                '    [ -n "$ok" ] && ipc_ok && hyprctl layers | grep -q \'rustshell:bar\' && { up=1; break; }\n'
            ),
            (
                '    [ -n "$ok" ] && ipc_ok && bar_up && { up=1; break; }\n'
                '    [ $mode = qt ] && [ $((SECONDS - t0)) -ge 20 ] && break\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                '  setstart "$QT_START" "$NAT_START"\n'
            ),
            (
                '  setstart "$NAT_START"\n'
            ),
        ),
        (
            PC_DEFAULT,
            (
                '[ $rc = 0 ] || [ $mode != qt ] || [ -z "$was_native" ] || back_to_native\n'
            ),
            (
                '[ $rc = 0 ] || [ $mode != qt ] || [ -z "$was_native" ] || back_to_native\n'
                '# mode is native here unless the Qt shell is up\n'
                'if [ $mode = native ]; then watch_again; elif [ -n "$watch" ]; then echo "$watch" > $WSAVED; fi\n'
            ),
        ),
        # lines that also give the credit, which no rule touches
        (
            "docs/original-differences.md",
            '("ii", end-4/dots-hyprland) and does what ii\'s\n',
            '("ii", end-4/dots-hyprland) and does what the original\'s\n',
        ),
        (
            "crates/qml/src/layout_model/rusty.rs",
            "(ii as installed by the end-4 dots, then customized)",
            "(the original as installed by the end-4 dots, then customized)",
        ),
        (
            "crates/qml/tests/upstream_fixes.rs",
            "when it was taken from ii (reported as end-4/dots-hyprland issues)",
            "when it was taken from the original (reported as end-4/dots-hyprland issues)",
        ),
        # The native shell is not reached by Quickshell's `qs` any more (its socket and its tree
        # are under its own names): the scripts call its own client, and say which shell they mean.
        (
            "tools/stress/common.sh",
            """# The shell under test and how `qs` addresses it. native:""",
            """# The shell under test and what addresses its IPC. native:""",
        ),
        (
            "tools/stress/common.sh",
            """# copy of this checkout's qml), `qs -c rusty-impulse`. orig:""",
            """# copy of this checkout's qml), `rusty-impulse -c rusty-impulse` (Quickshell's `qs` does not find it). orig:""",
        ),
        (
            "tools/stress/common.sh",
            """if [ "$MODE" = orig ]; then QS="qs -p $ORIG_TREE"; else QS="qs -c rusty-impulse"; fi
""",
            """if [ "$MODE" = orig ]; then QS="qs -p $ORIG_TREE" NS=quickshell; else QS="rusty-impulse -c rusty-impulse" NS=rustshell; fi
""",
        ),
        (
            "tools/stress/common.sh",
            """hglobal() { hyprctl dispatch "hl.dsp.global(\\"rustshell:$1\\")" >/dev/null; }""",
            """hglobal() { hyprctl dispatch "hl.dsp.global(\\"$NS:$1\\")" >/dev/null; }""",
        ),
        (
            "tools/stress/common.sh",
            """[ -n "$CLEAN" ] && rm -f ~/.cache/rustshell/notifications/notifications.json""",
            """[ -n "$CLEAN" ] && rm -f ~/.cache/$NS/notifications/notifications.json""",
        ),
        (
            "tools/stress/scr.sh",
            """else QS="qs -c rusty-impulse"; fi
""",
            """else QS="rusty-impulse -c rusty-impulse"; fi
""",
        ),
        (
            "tools/stress/longcycle.sh",
            """local live=$(qs -c rusty-impulse ipc call ri-debug dump""",
            """local live=$(rusty-impulse -c rusty-impulse ipc call ri-debug dump""",
        ),
        (
            "tools/stress/longcycle.sh",
            """do qs -c rusty-impulse ipc call $p toggle >/dev/null 2>&1; sleep 1.2; qs -c rusty-impulse ipc call $p toggle""",
            """do rusty-impulse -c rusty-impulse ipc call $p toggle >/dev/null 2>&1; sleep 1.2; rusty-impulse -c rusty-impulse ipc call $p toggle""",
        ),
        (
            "tools/stress/panelgrowth.sh",
            """tg() { qs -c rusty-impulse ipc call $T toggle >/dev/null 2>&1; sleep $D; qs -c rusty-impulse ipc call $T toggle""",
            """tg() { rusty-impulse -c rusty-impulse ipc call $T toggle >/dev/null 2>&1; sleep $D; rusty-impulse -c rusty-impulse ipc call $T toggle""",
        ),
        (
            "tools/stress/README.md",
            """(`common.sh`): `qs -c rusty-impulse` in
""",
            """(`common.sh`): `rusty-impulse -c rusty-impulse` in
""",
        ),
        (
            "tools/stress/README.md",
            """(written by `qs -c rusty-impulse ipc call ri-debug dump`""",
            """(written by `rusty-impulse -c rusty-impulse ipc call ri-debug dump`""",
        ),
        (
            "bench/README.md",
            """poll IPC (`qs -c rusty-impulse ipc call cliphistService update`)""",
            """poll IPC (`ipc call cliphistService update`, sent by `qs` to the original and by `rusty-impulse` to the native shell)""",
        ),
        (
            "tools/parity/README.md",
            """# tools/parity: native rusty-impulse against the original `qs -c rusty-impulse`, shot by shot""",
            """# tools/parity: native rusty-impulse against the original in Quickshell, shot by shot""",
        ),
        (
            "bench/first-open.sh",
            """#                      namespace quickshell:<PANEL>
""",
            """#                      namespace rustshell:<PANEL> (the original's: quickshell:<PANEL>)
""",
        ),
        (
            "tools/install.sh",
            """`rusty-impulse ipc call ...` is its Qt-free IPC client, and a
# real `qs -c rusty-impulse ipc call ...` reaches it too (it serves Quickshell's IPC socket). `pidof qs`
""",
            """`rusty-impulse ipc call ...` is its Qt-free IPC client. A
# real `qs` does not reach it: its socket is under $XDG_RUNTIME_DIR/rustshell, not Quickshell's. `pidof qs`
""",
        ),
        # The test that runs those scripts: its `rusty-impulse` answers IPC calls, its `qs` does not.
        (
            "crates/shell/tests/stress_scripts.rs",
            """//! as `~/h`, and `hyprctl`, `qs`, `pkill` and the other programs they call are scripts. The
//! shell under test is a process that does nothing.
""",
            """//! as `~/h`, and `hyprctl`, `pkill` and the other programs they call are scripts. The shell
//! under test is a process that does nothing; called as its own IPC client
//! (`rusty-impulse -c rusty-impulse ipc call ...`, which is how the scripts reach it) it answers.
""",
        ),
        (
            "crates/shell/tests/stress_scripts.rs",
            """        script(&bin.join("qs"), "#!/bin/bash\\nexit 0\\n");
""",
            """        // Quickshell's `qs` does not find the native shell: a script that asked it would wait.
        script(&bin.join("qs"), "#!/bin/bash\\nexit 255\\n");
""",
        ),
        (
            "crates/shell/tests/stress_scripts.rs",
            """        // Started, the shell has nothing to do.
        script(
            &bin.join("rusty-impulse"),
            "#!/bin/bash\\nflock -u 8 2>/dev/null\\nexec 8>&- 9>&-\\nexec tail -f /dev/null\\n",
        );
""",
            """        script(&bin.join("rusty-impulse"), &shell(""));
""",
        ),
        (
            "crates/shell/tests/stress_scripts.rs",
            """    script(
        &guest.bin().join("qs"),
        &format!(
            "#!/bin/bash\\n[ \\"$*\\" = '-c rusty-impulse ipc call lock activate' ] && echo activate >>{}\\nexit 0\\n",
            seen.display()
        ),
    );
""",
            """    script(
        &guest.bin().join("rusty-impulse"),
        &shell(&format!(
            "[ \\"$*\\" = '-c rusty-impulse ipc call lock activate' ] && echo activate >>{}\\n",
            seen.display()
        )),
    );
""",
        ),
        (
            "crates/shell/tests/stress_scripts.rs",
            """
struct Guest {""",
            """
/// The program `rusty-impulse`: its IPC client answers every call (after `on_call`, bash run
/// with the call's arguments); started with no arguments it is the shell, which has nothing to
/// do.
fn shell(on_call: &str) -> String {
    format!(
        "#!/bin/bash\\nif [ \\"$3\\" = ipc ]; then\\n{on_call}exit 0\\nfi\\n\\
         flock -u 8 2>/dev/null\\nexec 8>&- 9>&-\\nexec tail -f /dev/null\\n"
    )
}

struct Guest {""",
        ),
    ],
    1: [
        (
            "crates/qml/src/syntax/data/LICENSES.md",
            "is built into ii-rs:",
            "is built into rusty-impulse:",
        ),
        # a layout or a config written before the rename still names the family and the edge "ii"
        (
            "crates/qml/src/layout_model/format.rs",
            "    Right,\n    Ii,\n",
            '    Right,\n    #[serde(alias = "ii")]\n    Rusty,\n',
        ),
        (
            "qml/shell.qml",
            "        active: Config.ready && Config.options.panelFamily === identifier && extraCondition\n",
            "        property string oldIdentifier: \"\"\n"
            "        active: Config.ready && (Config.options.panelFamily === identifier || Config.options.panelFamily === oldIdentifier) && extraCondition\n",
        ),
        (
            "qml/shell.qml",
            '        identifier: "rusty"\n        component',
            '        identifier: "rusty"\n        oldIdentifier: "ii" // in a config written before the rename\n        component',
        ),
        # the list of variables has one prefix left to look for
        (
            "crates/qml/tests/env_registry.rs",
            '    for prefix in ["II_", "RI_"] {\n',
            '    for prefix in ["RI_"] {\n',
        ),
        (
            "crates/qml/tests/env_registry.rs",
            '    assert_eq!(v, ["RI_AB_LOG", "RI_ROOT", "RI_DEBUG", "RI_LOG"]);\n',
            '    assert_eq!(v, ["RI_AB_LOG", "RI_DEBUG", "RI_LOG", "RI_ROOT"]);\n',
        ),
        (
            "docs/env.md",
            "| `RI_EQUIV_SKIP` |",
            "| `RI_ROUNDS` | `crates/qml/tests/notifications.rs` | How many rounds of 500 notifications `flood_500` sends (4 without it). |\n"
            "| `RI_EQUIV_SKIP` |",
        ),
    ],
    2: [
        # Scenes Qt and we both read from a temporary tree: the tree's own files import
        # `ri.…`, so the directory both find them under is `ri`.
        (
            "crates/shell/tests/support/qt_window.rs",
            """    dprs: &[f64],
) {
    let (w, h) = size;
    let root = tree.join("qs");
""",
            """    dprs: &[f64],
) {
    let (w, h) = size;
    let root = tree.join("ri");
""",
        ),
        (
            "crates/shell/tests/support/qt_window.rs",
            """    off: &[Off],
) {
    let (w, h) = size;
    let root = tree.join("qs");
""",
            """    off: &[Off],
) {
    let (w, h) = size;
    let root = tree.join("ri");
""",
        ),
        (
            "crates/shell/tests/plain_layer_qt.rs",
            'let widgets = dir.path().join("qs/modules/common/widgets");',
            'let widgets = dir.path().join("ri/modules/common/widgets");',
        ),
        (
            "crates/shell/tests/plain_layer_qt.rs",
            '"module qs.modules.common.widgets\\nRoundCorner 1.0 RoundCorner.qml\\n"',
            '"module ri.modules.common.widgets\\nRoundCorner 1.0 RoundCorner.qml\\n"',
        ),
        (
            "crates/shell/tests/plain_layer_qt.rs",
            '"import qs.modules.common.widgets\\n",\n        &body,',
            '"import ri.modules.common.widgets\\n",\n        &body,',
        ),
        # tools/parity runs the original and this shell in one home. Each has its own names now:
        # its layers, its socket, and the directories the two shared, which setup.sh links.
        (
            "tools/parity/lib.sh",
            '  else timeout 5 qs -c rusty-impulse ipc call "$@" >/dev/null 2>&1; fi\n}\n',
            '  else timeout 5 "${BIN:-$HOME/rusty-impulse/target/release/rusty-impulse}" -c rusty-impulse ipc call "$@" >/dev/null 2>&1; fi\n'
            "}\n"
            "# What the shell under test puts before the names of its layers.\n"
            'ns() { if [ "${MODE:-}" = orig ]; then echo quickshell; else echo rustshell; fi; }\n',
        ),
        (
            "tools/parity/lib.sh",
            '--arg n "quickshell:$1" \'[.. | .namespace? // empty] | index($n)\'',
            '--arg n "$(ns):$1" \'[.. | .namespace? // empty] | index($n)\'',
        ),
        (
            "tools/parity/lib.sh",
            'namespace = \\"quickshell:$1\\" }, no_anim',
            'namespace = \\"$(ns):$1\\" }, no_anim',
        ),
        (
            "tools/parity/setup.sh",
            'hyprctl eval "hl.env(\\"RUSTY_IMPULSE_VIRTUAL_ENV\\",\\"$VENV\\")" >/dev/null\n',
            'hyprctl eval "hl.env(\\"RUSTY_IMPULSE_VIRTUAL_ENV\\",\\"$VENV\\")" >/dev/null\n'
            "# The original's scripts read it under the original's name.\n"
            'hyprctl eval "hl.env(\\"$(printf %s_%s_VIRTUAL_ENV ILLOGICAL IMPULSE)\\",\\"$VENV\\")" >/dev/null\n',
        ),
        (
            "bench/hyprland.lua",
            'hl.env("RUSTY_IMPULSE_VIRTUAL_ENV", "/opt/ri-venv")\n',
            'hl.env("RUSTY_IMPULSE_VIRTUAL_ENV", "/opt/ri-venv")\n'
            'hl.env("ILLOGICAL" .. "_IMPULSE_VIRTUAL_ENV", "/opt/ri-venv") -- the original\'s name for it\n',
        ),
        (
            "tools/parity/scenes.sh",
            '  cp "$PARITY/probe_qtr.qml" "$tree/probe_qtr.qml"\n',
            "  # The probe is written in the original's words; this shell gets it in its own.\n"
            '  if [ "$MODE" = native ]; then\n'
            "    sed -e 's/^import Quickshell/import Rustshell/' -e 's/^import qs[.]/import ri./' -e 's/[.]modules[.]i\\{2\\}[.]/.modules.rusty./' \\\n"
            '      "$PARITY/probe_qtr.qml" > "$tree/probe_qtr.qml"\n'
            '  else cp "$PARITY/probe_qtr.qml" "$tree/probe_qtr.qml"; fi\n',
        ),
        (
            "tools/parity/setup.sh",
            'mkdir -p "$(dirname "$CFG")" "$HOME/.local/state/quickshell/user"\n',
            'mkdir -p "$(dirname "$CFG")" "$HOME/.local/state/quickshell/user" "$HOME/.cache/quickshell"\n'
            "# The original and this shell read the same settings, colours and history here, as they\n"
            "# did while they had the same names for them.\n"
            'for d in "$HOME/.local/state" "$HOME/.cache"; do\n'
            '  [ -L "$d/rustshell" ] || rm -rf "$d/rustshell"\n'
            '  ln -sfn quickshell "$d/rustshell"\n'
            "done\n"
            'old=$HOME/.config/$(printf %s-%s illogical impulse)\n'
            '[ -L "$old" ] || rm -rf "$old"\n'
            'ln -sfn "$(basename "$(dirname "$CFG")")" "$old"\n',
        ),
        # The benchmarks that start either shell: a layer of either, and one set of files.
    ] + [
        (
            f,
            """layer() { hyprctl layers -j | jq -e --arg n "quickshell:$1" '[.. | .namespace? // empty] | index($n)' >/dev/null; }
""",
            """layer() { hyprctl layers -j | jq -e --arg n "$1" '[.. | .namespace? // empty] | (index("quickshell:" + $n) // index("rustshell:" + $n))' >/dev/null; }
""",
        )
        for f in ("bench/rss-cpu.sh", "bench/first-open.sh")
    ] + [
        (
            f,
            """    rm -rf ~/.config/rusty-impulse ~/.local/state/quickshell ~/.cache/quickshell ~/.config/matugen
    mkdir -p ~/.config/rusty-impulse ~/.local/state/quickshell/user
""",
            """    local old; old=~/.config/$(printf %s-%s illogical impulse)
    rm -rf ~/.config/rusty-impulse ~/.local/state/quickshell ~/.cache/quickshell ~/.config/matugen \\
        ~/.local/state/rustshell ~/.cache/rustshell "$old"
    mkdir -p ~/.config/rusty-impulse ~/.local/state/quickshell/user ~/.cache/quickshell
    # Each shell reads these under its own name for them.
    ln -s quickshell ~/.local/state/rustshell
    ln -s quickshell ~/.cache/rustshell
    ln -s rusty-impulse "$old"
""",
        )
        for f in ("bench/rss-cpu.sh", "bench/first-open.sh")
    ] + [
        # The notification benchmarks write the history both shells start with, ask the
        # shell under test through its own program and press its own shortcut.
        (
            f,
            """json.dump(d,open("/home/dar1k/.cache/quickshell/notifications/notifications.json","w"),indent=2)
PY
""",
            """json.dump(d,open("/home/dar1k/.cache/quickshell/notifications/notifications.json","w"),indent=2)
PY
# The native shell reads its history under its own name for the directory.
mkdir -p ~/.cache/rustshell/notifications && cp ~/.cache/quickshell/notifications/notifications.json ~/.cache/rustshell/notifications/
""",
        )
        for f in ("bench/vflood.sh", "bench/vswipe.sh", "bench/qtflood/vfperf.sh")
    ] + [
        (
            f,
            """ipc(){ local s=$(date +%s%N); timeout 300 qs $C ipc call""",
            """ipc(){ local s=$(date +%s%N); timeout 300 $Q $C ipc call""",
        )
        for f in ("bench/vflood.sh", "bench/qtflood/vfperf.sh")
    ] + [
        (
            "bench/vflood.sh",
            """[ "$1" = ORIG ] && { S='quickshell|qs'; C="-p $T"; } || { S=rusty-impulse; C="-c rusty-impulse"; }
""",
            """[ "$1" = ORIG ] && { S='quickshell|qs'; C="-p $T"; Q=qs; } || { S=rusty-impulse; C="-c rusty-impulse"; Q=${BIN:-target/release/rusty-impulse}; }
""",
        ),
        (
            "bench/qtflood/vfperf.sh",
            """[ "$M" = ORIG ] && { S='quickshell|qs'; C="-p $T"; } || { S=rusty-impulse; C="-c rusty-impulse"; }
""",
            """[ "$M" = ORIG ] && { S='quickshell|qs'; C="-p $T"; Q=qs; } || { S=rusty-impulse; C="-c rusty-impulse"; Q=${BIN:-$HOME/bin/rusty-impulse}; }
""",
        ),
        (
            "bench/vswipe.sh",
            """G(){ hyprctl eval "return hl.dispatch(hl.dsp.global(\\"quickshell:$1\\"))" >/dev/null; }
""",
            """G(){ hyprctl eval "return hl.dispatch(hl.dsp.global(\\"$NS:$1\\"))" >/dev/null; }
""",
        ),
        (
            "bench/vswipe.sh",
            """[ "$1" = ORIG ] && S='quickshell|qs' || S=rusty-impulse
""",
            """[ "$1" = ORIG ] && { S='quickshell|qs'; NS=quickshell; } || { S=rusty-impulse; NS=rustshell; }
""",
        ),
    ] + [
        (
            "bench/first-open.sh",
            """subprocess.run(["hyprctl", "dispatch", f'hl.dsp.global("quickshell:{shortcut}")'],
""",
            """subprocess.run(["hyprctl", "dispatch", f'hl.dsp.global("{os.environ["NS"]}:{shortcut}")'],
""",
        ),
        (
            "bench/first-open.sh",
            """event_ms() {
    python3 - "$1" "$2" <<'PY'
""",
            """event_ms() {
    NS=$ns python3 - "$1" "$2" <<'PY'
""",
        ),
        (
            "bench/first-open.sh",
            """open_close() {
    event_ms "${PANEL}Open" "openlayer>>quickshell:$PANEL" || die "$shell: $PANEL did not open"
    sleep 3
    event_ms "${PANEL}Close" "closelayer>>quickshell:$PANEL" >/dev/null || die "$shell: $PANEL did not close"
""",
            """open_close() {
    # What the shell under test puts before the names of its shortcuts and of its layers.
    local ns=rustshell
    [ "$shell" != quickshell ] || ns=quickshell
    event_ms "${PANEL}Open" "openlayer>>$ns:$PANEL" || die "$shell: $PANEL did not open"
    sleep 3
    event_ms "${PANEL}Close" "closelayer>>$ns:$PANEL" >/dev/null || die "$shell: $PANEL did not close"
""",
        ),
        # The lookup test gives both shells one set of directories: Quickshell's, which this
        # shell reads under its own name through a link, and says under Quickshell's.
        (
            "crates/shell/tests/config_lookup_equiv.rs",
            """        std::fs::write(&p, "").unwrap();
        p
""",
            """        std::fs::write(&p, "").unwrap();
        // This shell reads the same directory under its own name.
        let theirs = p.ancestors().find(|a| a.ends_with("quickshell")).unwrap();
        let _ = std::os::unix::fs::symlink("quickshell", theirs.with_file_name(OWN));
        p
""",
        ),
        (
            "crates/shell/tests/config_lookup_equiv.rs",
            """        (format!("{out}{err}"), o.status.code())
""",
            """        let mut said = format!("{out}{err}");
        if bin == Path::new(NATIVE) {
            said = said.replace(&format!("/{OWN}/"), "/quickshell/");
        }
        (said, o.status.code())
""",
        ),
        (
            "crates/shell/tests/config_lookup_equiv.rs",
            """const NATIVE: &str = env!("CARGO_BIN_EXE_rusty-impulse");
""",
            """const NATIVE: &str = env!("CARGO_BIN_EXE_rusty-impulse");
/// What this shell calls the directory Quickshell calls `quickshell`.
const OWN: &str = "%s";
""" % ENGINE,
        ),
        # a test of this shell alone in a file kept in real Quickshell's words: the namespace
        # of a layer that names none
        (
            "crates/shell/tests/windows_outputs.rs",
            'fn keep_awake_after_unplug(only: bool) {\n    const NS: &str = "quickshell";\n',
            'fn keep_awake_after_unplug(only: bool) {\n    const NS: &str = "%s";\n' % ENGINE,
        ),
        # The tests that give the shell's own tree to real Quickshell give it a copy in
        # Quickshell's words; this shell reads the copy too (see types.rs below).
        (
            "crates/testenv/src/impulse_qt.rs",
            "/// One `Name { key: value ... Child { } }` of a `.qmltypes` file.\n",
            "/// Copies the shell's tree `from` into `to` in real Quickshell's words. The tree names the\n"
            "/// engine's module and the prefix of its own directories as this engine does, and Quickshell\n"
            "/// knows neither name. A build with the interpreter reads both, so a comparison gives the\n"
            "/// copy to both shells.\n"
            "pub fn tree_in_quickshells_words(from: &Path, to: &Path) {\n"
            "    std::fs::create_dir_all(to).unwrap();\n"
            "    for e in std::fs::read_dir(from).unwrap() {\n"
            "        let e = e.unwrap();\n"
            "        let (src, dst) = (e.path(), to.join(e.file_name()));\n"
            "        if std::fs::metadata(&src).unwrap().is_dir() {\n"
            "            tree_in_quickshells_words(&src, &dst);\n"
            "            continue;\n"
            "        }\n"
            "        let code = src\n"
            "            .extension()\n"
            "            .is_some_and(|x| x == \"qml\" || x == \"js\" || x == \"mjs\");\n"
            "        match std::fs::read_to_string(&src) {\n"
            "            Ok(text) if code => std::fs::write(&dst, in_quickshells_words(&text)).unwrap(),\n"
            "            _ => {\n"
            "                std::fs::copy(&src, &dst).unwrap();\n"
            "            }\n"
            "        }\n"
            "    }\n"
            "}\n"
            "\n"
            "/// A file of the shell's tree in real Quickshell's words: its module, and the prefix of the\n"
            "/// tree's own directories in an import.\n"
            "pub fn in_quickshells_words(text: &str) -> String {\n"
            "    whole_word(\n"
            "        &whole_word(text, \"Rustshell\", \"Quickshell\"),\n"
            "        \"import ri\",\n"
            "        \"import qs\",\n"
            "    )\n"
            "}\n"
            "\n"
            "/// `text` with `to` where `from` stands as a word of its own.\n"
            "fn whole_word(text: &str, from: &str, to: &str) -> String {\n"
            "    let in_a_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');\n"
            "    let mut out = String::with_capacity(text.len());\n"
            "    let mut rest = text;\n"
            "    while let Some(i) = rest.find(from) {\n"
            "        out.push_str(&rest[..i]);\n"
            "        let after = &rest[i + from.len()..];\n"
            "        let whole = !in_a_word(out.chars().next_back()) && !in_a_word(after.chars().next());\n"
            "        out.push_str(if whole { to } else { from });\n"
            "        rest = after;\n"
            "    }\n"
            "    out.push_str(rest);\n"
            "    out\n"
            "}\n"
            "\n"
            "/// One `Name { key: value ... Child { } }` of a `.qmltypes` file.\n",
        ),
        (
            "crates/testenv/src/lib.rs",
            "pub use impulse_qt::impulse_qt_module;\n",
            "pub use impulse_qt::{impulse_qt_module, in_quickshells_words, tree_in_quickshells_words};\n",
        ),
        (
            "crates/shell/tests/image_status_qt.rs",
            """    let ii = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let d = tempfile::tempdir().unwrap();
    let st = Command::new("cp")
        .arg("-r")
        .arg(ii.join("."))
        .arg(d.path())
        .status()
        .unwrap();
    assert!(st.success());
""",
            """    let tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let d = tempfile::tempdir().unwrap();
    testenv::tree_in_quickshells_words(&tree, d.path());
""",
        ),
        (
            "crates/shell/tests/interact.rs",
            """    let st = Command::new("cp")
        .arg("-r")
        .arg(root.join("qml/."))
        .arg(dir.path())
        .status()
        .unwrap();
    assert!(st.success());
""",
            """    testenv::tree_in_quickshells_words(&root.join("qml"), dir.path());
""",
        ),
        (
            "crates/shell/tests/interact.rs",
            """    std::fs::copy(
        root.join("crates/qml/tests/fixtures/original-unshipped")
            .join(spin),
        dir.path().join(spin),
    )
    .unwrap();
""",
            """    let text = std::fs::read_to_string(
        root.join("crates/qml/tests/fixtures/original-unshipped")
            .join(spin),
    )
    .unwrap();
    std::fs::write(dir.path().join(spin), testenv::in_quickshells_words(&text)).unwrap();
""",
        ),
        (
            "crates/shell/tests/session_lock_reload.rs",
            """    let st = Command::new("cp")
        .arg("-r")
        .arg(root.join("qml/."))
        .arg(dir.path())
        .status()
        .unwrap();
    assert!(st.success());
""",
            """    testenv::tree_in_quickshells_words(&root.join("qml"), dir.path());
""",
        ),
        (
            "crates/shell/tests/plain_layer_qt.rs",
            """    let ii = here.join("../../qml").canonicalize().unwrap();
    // Links to all of `from` but `not`, which is returned to be filled.
    let linked = |from: &Path, to: &Path, not: &str| {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_name() != not {
                std::os::unix::fs::symlink(e.path(), to.join(e.file_name())).unwrap();
            }
        }
        (from.join(not), to.join(not))
    };
    let (from, to) = linked(&ii, dir, "modules");
    let (from, to) = linked(&from, &to, "common");
    let (from, to) = linked(&from, &to, "models");
    let (_, model) = linked(&from, &to, "WorkspaceModel.qml");
""",
            """    testenv::tree_in_quickshells_words(&here.join("../../qml"), dir);
    let model = dir.join("modules/common/models/WorkspaceModel.qml");
""",
        ),
        # The tests that asked both shells through Quickshell's `qs`: a shell finds the
        # instances of its own name, so each is asked through its own program.
        (
            "crates/shell/tests/destruction_equiv.rs",
            """        let o = Command::new("qs")
            .arg("-p")
            .arg(&conf)
            .arg("kill")
""",
            """        // Each shell is asked through its own program: it finds the instances of its name.
        let o = Command::new(shell(&conf).get_program())
            .arg("-p")
            .arg(&conf)
            .arg("kill")
""",
        ),
        (
            "crates/shell/tests/destruction_equiv.rs",
            """    let kill = |_: &Child, conf: &Path| {
        let o = Command::new("qs")
""",
            """    let kill = |shell: &Child, conf: &Path| {
        // Each shell is asked through its own program: it finds the instances of its name.
        let own = std::fs::read_link(format!("/proc/{}/exe", shell.id())).unwrap();
        let o = Command::new(own)
""",
        ),
        (
            "crates/shell/tests/ipc_equiv.rs",
            """fn qs(dir: &Path, args: &[&str]) -> Output {
""",
            """/// Quickshell's `qs` finds an instance in Quickshell's part of the runtime directory, and
/// the native shell registers in its own. The tests here ask both through `qs`, so the native
/// instance `pid` is entered in Quickshell's part too, under the path of its config.
fn known_to_qs(pid: u32) {
    let rt = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    let own = rt.join("%s");
    let id = std::fs::read_link(own.join("by-pid").join(pid.to_string())).unwrap();
    let id = id.file_name().unwrap().to_owned();
    let theirs = rt.join("quickshell");
    for d in ["by-id", "by-path"] {
        std::fs::create_dir_all(theirs.join(d)).unwrap();
    }
    let link = |to: std::path::PathBuf, at: std::path::PathBuf| {
        let _ = std::os::unix::fs::symlink(to, at);
    };
    link(own.join("by-id").join(&id), theirs.join("by-id").join(&id));
    for e in std::fs::read_dir(own.join("by-path")).unwrap() {
        let e = e.unwrap();
        if e.path().join(&id).exists() {
            link(e.path(), theirs.join("by-path").join(e.file_name()));
        }
    }
}

fn qs(dir: &Path, args: &[&str]) -> Output {
""" % ENGINE,
        ),
        (
            "crates/shell/tests/ipc_equiv.rs",
            """        .unwrap_or_else(|l| panic!("{bin} never loaded the fixture:\\n{l}"));
""",
            """        .unwrap_or_else(|l| panic!("{bin} never loaded the fixture:\\n{l}"));
    if bin != "qs" {
        known_to_qs(s.proc.0.id());
    }
""",
        ),
        # The sway of a test floats the windows of both shells.
        (
            "crates/shell/tests/scale_factor_equiv.rs",
            """        "for_window [app_id=\\"org.quickshell\\"] floating enable",
""",
            """        "for_window [app_id=\\"org.quickshell\\"] floating enable",
        "for_window [app_id=\\"org.%s\\"] floating enable",
""" % ENGINE,
        ),
        # The registry and the logs one test writes for both shells: this shell reads them
        # under its own name through a link.
        (
            "crates/shell/tests/qs_list_log.rs",
            """    let t_dir = cfg.join("quickshell/t");
    std::fs::create_dir_all(&t_dir).unwrap();
""",
            """    let t_dir = cfg.join("quickshell/t");
    std::fs::create_dir_all(&t_dir).unwrap();
    // The native shell reads the same directories under its own name.
    std::fs::create_dir_all(&rt).unwrap();
    for d in [&cfg, &rt] {
        std::os::unix::fs::symlink("quickshell", d.join("%s")).unwrap();
    }
""" % ENGINE,
        ),
        (
            "crates/shell/tests/qs_list_log.rs",
            """            let got = self.run(Path::new(NATIVE), args, extra);
""",
            """            let (out, err, code) = self.run(Path::new(NATIVE), args, extra);
            // The native shell names the directories by its own name for them.
            let theirs = |s: String| s.replace("/%s/", "/quickshell/");
            let got = (theirs(out), theirs(err), code);
""" % ENGINE,
        ),
        (
            "crates/shell/tests/qs_list_log.rs",
            """    let _ = symlink(&shell, rt.join("by-path").join(&path_id));
""",
            """    let _ = symlink(&shell, rt.join("by-path").join(&path_id));
    // The native shell finds the same config under its own name for the directory.
    let own = md5_hex(&s.config.replace("/quickshell/", "/%s/"));
    let _ = symlink(&shell, rt.join("by-path").join(own));
""" % ENGINE,
        ),
        (
            "crates/shell/tests/qs_list_log.rs",
            """    assert_eq!(ids.len(), 1, "{ids:?}");
    let qslog = rt.join("quickshell/by-id")""",
            """    assert_eq!(ids.len(), 1, "{ids:?}");
    // The native shell finds the same config under its own name for the directory.
    let by_path = rt.join("quickshell/by-path");
    let theirs = dir.join("shell.qml").to_string_lossy().into_owned();
    let _ = symlink(
        by_path.join(md5_hex(&theirs)),
        by_path.join(md5_hex(&theirs.replace("/quickshell/", "/%s/"))),
    );
    let qslog = rt.join("quickshell/by-id")""" % ENGINE,
        ),
        (
            "crates/shell/tests/qs_list_log.rs",
            """    let rt = root.join("qt-rt");
    let dir = root.join("cfg/quickshell/w");
""",
            """    let rt = root.join("qt-rt");
    // The native shell reads what Quickshell writes there under its own name.
    std::fs::create_dir_all(&rt).unwrap();
    std::os::unix::fs::symlink("quickshell", rt.join("%s")).unwrap();
    let dir = root.join("cfg/quickshell/w");
""" % ENGINE,
        ),
        # a test kept in real Quickshell's words that uses a module of the shell by its name
        (
            "crates/shell/tests/qs_list_log.rs",
            "use shell::qslog::",
            "use shell::rilog::",
        ),
        # The tests that run a file in both shells keep it in real Quickshell's words. Only a
        # build with the interpreter (tests, parity tooling) reads those; the shipped one has none.
        (
            "crates/qml/src/types.rs",
            "pub fn registry() -> &'static Registry {\n    REGISTRY.get_or_init(Registry::load)\n}\n",
            "pub fn registry() -> &'static Registry {\n    REGISTRY.get_or_init(Registry::load)\n}\n"
            "\n"
            "/// What real Quickshell calls its module and its singleton. The tests that run a file in\n"
            "/// both shells write it in those words, so a build with the interpreter reads them as this\n"
            "/// engine's own names; the shipped build has no interpreter and knows only its own.\n"
            "#[cfg(feature = \"interp\")]\n"
            "const THEIRS: &str = concat!(\"Quick\", \"shell\");\n"
            "\n"
            "/// The uri of an import written for real Quickshell (its module, the directories of the\n"
            "/// tree after its prefix), as this engine names it.\n"
            "#[cfg(feature = \"interp\")]\n"
            "pub fn own_uri(uri: String) -> String {\n"
            "    for (theirs, ours) in [(THEIRS, \"Rustshell\"), (concat!(\"q\", \"s\"), \"ri\")] {\n"
            "        match uri.strip_prefix(theirs) {\n"
            "            Some(rest) if rest.is_empty() || rest.starts_with('.') => {\n"
            "                return format!(\"{ours}{rest}\");\n"
            "            }\n"
            "            _ => {}\n"
            "        }\n"
            "    }\n"
            "    uri\n"
            "}\n"
            "\n"
            "/// The name of a type written for real Quickshell (its singleton), as this engine names it.\n"
            "#[cfg(feature = \"interp\")]\n"
            "pub fn own_type(name: &str) -> &str {\n"
            "    if name == THEIRS { \"Rustshell\" } else { name }\n"
            "}\n",
        ),
        (
            "crates/qml/src/qqmljs/build.rs",
            "                    None => self.symbol(2).split_whitespace().collect(),\n                };\n",
            "                    None => self.symbol(2).split_whitespace().collect(),\n                };\n"
            "                #[cfg(feature = \"interp\")]\n"
            "                let uri = match file {\n"
            "                    Some(_) => uri,\n"
            "                    None => crate::types::own_uri(uri),\n"
            "                };\n",
        ),
        (
            "crates/qml/src/runtime.rs",
            "    fn type_value_of(&mut self, name: &str, templates: bool) -> Option<Value> {\n",
            "    fn type_value_of(&mut self, name: &str, templates: bool) -> Option<Value> {\n"
            "        #[cfg(feature = \"interp\")]\n"
            "        let name = crate::types::own_type(name);\n",
        ),
    ],
}



# What pass 2 changes in the scripts that run the original beside this shell and are otherwise
# left in real Quickshell's words: after pass 1 this is the path of this shell's tree only.
ALWAYS2 = [(r"quickshell/rusty-impulse\b", ENGINE + "/rusty-impulse")]


# What pass 3 leaves as it is besides SKIP: copies of the original's files, and what was
# measured or written down once and is a record.
SKIP3 = (
    "crates/qml/tests/fixtures/original",
    "crates/rigen/check/cases/",
    "LICENSE",
    "tools/parity/results/",
    "tools/parity/audits/",
)


def rules4():
    old = "rusty-impulse"
    return [
        # what the screen shows: window titles, the About page, the welcome window, in every language
        (r'title: "' + old + '"', 'title: "' + SHOWN + '"', r"^qml/"),
        (old + r" (?=Settings\b|Welcome\b)", SHOWN + " "),
        (old, SHOWN, r"^qml/translations/"),
        (r"\| " + old + "`", "| " + SHOWN + "`", r"^qml/modules/settings/About\.qml$"),
        (r"\(" + old + r"\)\]\(", "(" + SHOWN + ")]("),
        # the script that starts the shell at login
        (old + r"-shell\b", NEW + "-start"),
        (r"\$NEW_BIN-shell\b", "$NEW_BIN-start", r"^tools/carry-over\.sh$"),
        # the article before the name
        (r"\b([Aa]) (`?)" + old + r"\b", r"\1n \2" + NEW),
        # the name itself, as a program, a directory, a variable, a type, a function; not in
        # the repository's address on GitHub, which keeps it (REPO_ADDRESS)
        (r"(?<!github\.com/dar1k-ness/)" + old, NEW),
        (r"RUSTY_IMPULSE", NEW.upper().replace("-", "_")),
        (r"RustyImpulse", "".join(w.capitalize() for w in NEW.split("-"))),
        (r"rustyImpulse", NEW.split("-")[0] + "".join(w.capitalize() for w in NEW.split("-")[1:])),
        (r"rusty_impulse", NEW.replace("-", "_")),
    ]


def paths4():
    return [
        (r"rusty-impulse-shell$", NEW + "-start"),
        (r"rusty-impulse", NEW),
        (r"RustyImpulse", "".join(w.capitalize() for w in NEW.split("-"))),
        (r"rusty_impulse", NEW.replace("-", "_")),
    ]


def renamed4(text, rel=""):
    """`text` as pass 4 leaves it: what a hand edit of passes 1 to 3 reads in a tree renamed twice."""
    for rx, rep, *only in rules4():
        if not only or re.search(only[0], rel):
            text = re.sub(rx, rep, text)
    return text


def renamed4_path(rel):
    for rx, rep in paths4():
        rel = re.sub(rx, rep, rel)
    return rel


def rules(stage):
    if stage == 4:
        return rules4()
    if stage == 3:
        return LEFT
    if stage == 1:
        return [
            # the project's name
            (r"ILLOGICAL_IMPULSE", "RUSTY_IMPULSE"),
            (r"IllogicalImpulse", "RustyImpulse"),
            (r"illogicalImpulse", "rustyImpulse"),
            (r"illogical-impulse", "rusty-impulse"),
            # the binary and the variables named after it
            (r"\bHEAD_II_RS\b", "HEAD_RI_BIN"),
            (r"\bBASE_II_RS\b", "BASE_RI_BIN"),
            (r"\bII_RS_COMMIT\b", "RI_COMMIT"),
            (r"\bII_RS\b", "RI_BIN"),
            (r"\bIIRS_ROUNDS\b", "RI_ROUNDS"),
            (r"ii-rs", "rusty-impulse"),
            (r"\bii_rs\b", "rusty_impulse"),
            # the crates and the programs in them
            (r"iicompiled", "ricompiled"),
            (r"IICOMPILED", "RICOMPILED"),
            (r"iigen", "rigen"),
            (r"IIGEN", "RIGEN"),
            (r"iiflood", "riflood"),
            (r"IIFLOOD", "RIFLOOD"),
            (r"iiscen", "riscen"),
            (r"IISCEN", "RISCEN"),
            (r"iiaotcheck", "riaotcheck"),
            # the test environment and the gate
            (r"ii-gate", "ri-gate"),
            (r"ii-testenv", "ri-testenv"),
            (r"ii-venv", "ri-venv"),
            (r"ii-differences", "original-differences"),
            # every other II_ variable
            (r"\bII_([A-Z][A-Z0-9_]*)", r"RI_\1"),
            (r'strip_prefix\("II_"\)', 'strip_prefix("RI_")'),
            # the QML module directory
            (r"modules/ii\b", "modules/rusty"),
            (r"modules\.ii\b", "modules.rusty"),
            (r"M_modules_ii_", "M_modules_rusty_"),
            (r"\bIiBar\b", "RustyBar"),
            (r"\biiBarEdge", "rustyBarEdge"),
            (r"\biiPopupParent\b", "rustyPopupParent"),
            (r"\bIiLatexRenderer\b", "RustyLatexRenderer"),
            (r"\bii_configs\b", "rusty_configs"),
            # the config tree: ~/.config/quickshell/ii, `-c ii`, releases/<id>/ii
            (r"quickshell/ii\b", "quickshell/rusty-impulse"),
            (r'"-c", "ii"', '"-c", "rusty-impulse"'),
            (r"-c ii\b", "-c rusty-impulse", "^(?!" + ORIGINAL_COMMAND[1:-1] + "$)"),
            (r"(?<!\bqs )(?<!\bquickshell )-c ii\b", "-c rusty-impulse", ORIGINAL_COMMAND),
            (r'Some\("ii"', 'Some("rusty-impulse"'),
            # the Rust module of the layout the shell makes from its own options
            (r"\bmod ii\b", "mod rusty"),
            (r"\bii::(?=[A-Za-z_])", "rusty::"),
            (r"layout_model::ii\b", "layout_model::rusty"),
            (r"^(\s*use .*)\bii\b", r"\1rusty"),
            # the tree inside a release: `rusty-impulse` there is the binary
            (
                r"(\$stage|\$target|\$lib/releases/\$(?:cur|id|prev)|releases/\[!\.\]\*)/ii\b",
                r"\1/tree",
                INSTALL,
            ),
            (r'\b(current|previous|release_a|release_b|unwrap\(\))\.join\("ii"\)', r'\1.join("tree")', INSTALL),
            (r'part\.as_os_str\(\) == "ii"', 'part.as_os_str() == "tree"', INSTALL),
            (r"rusty-impulse and ii/, a copy", "rusty-impulse and tree/, a copy", INSTALL),
            (r'join\("ii"\)', 'join("rusty-impulse")'),
            # what the shell calls itself when it runs in a script's or a child's place
            (r"__ii-(?=[a-z])", "__ri-"),
            (r"\bInputs::ii\b", "Inputs::ri"),
            (r'args\("-cii ', 'args("-crusty-impulse '),
            (r"\\nii tree: \{\}", r"\\nQML tree: {}"),
            (r"no-such-user-ii\b", "no-such-user-ri"),
            (r'\)" != ii \]', ')" != rusty-impulse ]', r"^tools/testenv/power-cut$"),
            (r"\(default config: ii\)", "(default config: rusty-impulse)"),
            (r"/tmp/iirs\.tgz", "/tmp/rusty-impulse.tgz"),
            # the config's name where a test or a script spells it out
            (r"\$\{qsConfig:-ii\}", "${qsConfig:-rusty-impulse}"),
            (r'\\"ii\\" config', r'\\"rusty-impulse\\" config'),
            (r"--config=ii\b", "--config=rusty-impulse"),
            (r"/x/ii\b", "/x/rusty-impulse"),
            (r"/opt/(my )?ii/", r"/opt/\1rusty-impulse/"),
            (r"\b(dots|dotfiles)/ii\b", r"\1/rusty-impulse"),
            (r'"ii/(?=modules/|scripts/|services/|shell\.qml|")', '"rusty-impulse/'),
            (r"/ii/(?=modules/|Bar\.qml)", "/rusty-impulse/", r"^crates/shell/tests/shipped_panels\.rs$"),
            # the module directory as a test names it
            (r'"ii/(?=[A-Za-z])', '"rusty/', r"^crates/(aotcheck/tests/aot_check|shell/tests/progress_bar_qt)\.rs$"),
            (r"### ii/test\b", "### rusty/test"),
            (r"\bnotifications_ii\b", "notifications_rusty"),
            # what a test's stand-in notification server calls its vendor
            (r'"ii"(\.into\(\))?, "1"', r'"ri"\1, "1"'),
            (r'\["ii"\]', '["rusty-impulse"]'),
            (r'QUICKSHELL_CONFIG_NAME="ii"', 'QUICKSHELL_CONFIG_NAME="rusty-impulse"'),
            (r'"ii" config directory', '"rusty-impulse" config directory'),
            # the report sections and scenes of aotcheck
            (r'"modules", "ii"', '"modules", "rusty"'),
            (r'format!\("ii/', 'format!("rusty/'),
            (r"`ii/bar`", "`rusty/bar`"),
            (r'"ii", "ii-vertical"', '"rusty", "rusty-vertical"'),
            (r"\bii-vertical\b", "rusty-vertical"),
            (r'name: "ii"', 'name: "rusty"'),
            # the copies of the original kept for the tests, and the original tree of tools/parity
            (r"\bii-original\b", "original"),
            (r"\bii-unshipped\b", "original-unshipped"),
            (r"\bparity-ii\b", "parity-original"),
            # names made with the old prefix: temporary files, IPC targets, lock files
            (r"\bii-(?!qs\b)(?=[A-Za-z0-9{$])", "ri-"),
            (r"__ii_super", "__ri_super"),
            # the panel family and the bar edge stored in the user's files
            (r"RawEdge::Ii\b", "RawEdge::Rusty"),
            (r'\["panelFamily"\], "ii"', '["panelFamily"], "rusty"'),
            # (the comparison writes one config for both shells, in the original's word)
            (r'(panelFamily"?\s*[:=]+\s*)"ii"', r'\1"rusty"', r"^(?!tools/parity/)"),
            (r'(panelFamily \'?)"ii"', r'\1"rusty"', r"^(?!tools/parity/)"),
            (r'\["ii", "waffle"\]', '["rusty", "waffle"]'),
            (r'// "ii", "waffle"', '// "rusty", "waffle"'),
            (r'identifier: "ii"', 'identifier: "rusty"'),
            (r'edge = "ii"', 'edge = "rusty"'),
            (r'edge = \\"ii\\"', r'edge = \\"rusty\\"'),
            (r'`"ii"` for', '`"rusty"` for'),
        ]
    cap = ENGINE.capitalize()
    types = r"^crates/qml/qmltypes/"
    engine = r"^crates/(qml|shell|aotcheck|rigen|ricompiled-gen|ricompiled-rt)/src/"
    return [
        # the QML module and what the engine calls it
        (r"\bimport Quickshell\b", "import " + cap),
        (r"\bmodule Quickshell\b", "module " + cap),
        (r"\bQuickshell\.(?=[A-Za-z_])", cap + "."),
        (r"(?<=[\"'`])Quickshell(?=[\"'`/])", cap),
        (r"\bQuickshellGlobal\b", cap + "Global"),
        # the singleton itself, where a Connections listens to it
        (r"\btarget: Quickshell\b", "target: " + cap),
        (r"Quickshell", cap, types),
        (r"quickshell", ENGINE, types),
        # the prefix of the tree's own directories: `import qs.modules.common`
        (r"(?<=import )qs\b", "ri"),
        (r"\bmodule qs\b", "module ri"),
        (r"\bqs\.(?=(modules|services)\b)", "ri."),
        (r'"qs(?=\.?")', '"ri', r"^crates/qml/src/(qualified|resolve|runtime)\.rs$"),
        # the directories under ~/.config, ~/.local/state, ~/.cache, /tmp and $XDG_RUNTIME_DIR
        (r"/quickshell\b(?!-|\.txt)", "/" + ENGINE),
        # the layer namespaces, the window's application id, the D-Bus paths
        (r"\bquickshell:(?!/)", ENGINE + ":"),
        (r"\borg\.quickshell\b", "org." + ENGINE),
        # the namespace of a layer that names none, in what a test expects
        (r"(?<=\d \d )quickshell(?= (true|false))", ENGINE, r"^crates/qml/src/runtime\.rs$"),
        # (a category of Quickshell's own log keeps its name)
        (r'(?<!"qs", )(?<!starts_with\(b)"quickshell(?=["/])', '"' + ENGINE, r"^crates/"),
        # the directory a test lays the tree's own files out in, named after the prefix
        (r'"qs(?=["/])', '"ri', r"^crates/(qml/tests/(handler_values|hyprland_qml)|shell/tests/styled_text_dpr_qt)\.rs$"),
        (r"'quickshell'", "'" + ENGINE + "'", r"^crates/"),
        (r"\bQuickshell(?=ScreenInfo)", cap),
        (r"\bQuickshell\b", cap, r"^(docs/unwritten-methods\.tsv|crates/qml/src/enums\.tsv)$"),
        # the modules of the command line and of the log
        (r"\bqscli\b", "ricli"),
        (r"(?<!\.)\bqslog\b", "rilog"),
    ]

def paths(stage):
    if stage == 4:
        return paths4()
    if stage == 3:
        return []
    if stage == 1:
        return [
            (r"(^|/)iicompiled", r"\1ricompiled"),
            (r"(^|/)iigen(/|$)", r"\1rigen\2"),
            (r"(^|/)iiflood\.rs$", r"\1riflood.rs"),
            (r"(^|/)iiscen\.rs$", r"\1riscen.rs"),
            (r"(^|/)modules/ii/", r"\1modules/rusty/"),
            (r"(^|/)IllogicalImpulse", r"\1RustyImpulse"),
            (r"^crates/qml/src/layout_model/ii\.rs$", "crates/qml/src/layout_model/rusty.rs"),
            (r"^crates/qml/src/layout_model/ii_configs/", "crates/qml/src/layout_model/rusty_configs/"),
            (r"(^|/)notifications_ii\.rs$", r"\1notifications_rusty.rs"),
            (r"(^|/)ii-gate", r"\1ri-gate"),
            (r"^docs/ii-differences\.md$", "docs/original-differences.md"),
            (r"/\.config/illogical-impulse(/|$)", r"/.config/rusty-impulse\1"),
            (r"^crates/qml/tests/fixtures/ii-original/", "crates/qml/tests/fixtures/original/"),
            (r"^crates/qml/tests/fixtures/ii-unshipped/", "crates/qml/tests/fixtures/original-unshipped/"),
        ]
    cap = ENGINE.capitalize()
    return [
        (r"^(crates/qml/qmltypes/.*)quickshell-", r"\1" + ENGINE + "-"),
        (r"(^|/)Quickshell/", r"\1" + cap + "/"),
        (r"(handler_values/placement)/qs/", r"\1/ri/"),
        (r"/\.local/state/quickshell/", "/.local/state/" + ENGINE + "/"),
        (r"^crates/shell/src/qscli\.rs$", "crates/shell/src/ricli.rs"),
        (r"^crates/shell/src/qslog\.rs$", "crates/shell/src/rilog.rs"),
    ]

def tracked():
    try:
        out = subprocess.run(
            ["git", "-C", ROOT, "ls-files", "-z"], capture_output=True, check=True
        ).stdout
        return [p.decode() for p in out.split(b"\0") if p]
    except (subprocess.CalledProcessError, FileNotFoundError):
        # Not a checkout git can read (a worktree mounted without its repository): every
        # file there is, but for what a build leaves.
        found = []
        for top, dirs, files in os.walk(ROOT):
            dirs[:] = sorted(d for d in dirs if d not in (".git", "target"))
            found += [os.path.relpath(os.path.join(top, f), ROOT) for f in sorted(files) if f != ".git"]
        return found

def after_pass_1():
    """What follows from the renamed crates: the part crates, Cargo.lock, the formatting."""
    for d in ("crates/iicompiled", "crates/iicompiled-gen", "crates/iicompiled-rt", "crates/iigen", "qml/modules/ii",
              "crates/qml/tests/fixtures/ii-original", "crates/qml/tests/fixtures/ii-unshipped"):
        for top, _, _ in os.walk(os.path.join(ROOT, d), topdown=False):
            if not os.listdir(top):
                os.rmdir(top)
    subprocess.run(["bash", "mkparts.sh"], cwd=os.path.join(ROOT, "crates/ricompiled/parts"), check=True, stdout=subprocess.DEVNULL)
    lock = os.path.join(ROOT, "Cargo.lock")
    with open(lock) as f:
        text = f.read()
    text = text.replace("iicompiled", "ricompiled").replace('"iigen"', '"rigen"')
    with open(lock, "w") as f:
        f.write(text)
    # cargo puts the renamed packages where their new names sort
    subprocess.run(["cargo", "metadata", "--format-version", "1"], cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["cargo", "fmt", "--all"], cwd=ROOT, check=True)

# The repository on GitHub keeps the name pass 4 replaced: the address install.sh clones and
# docs/install.md gives is not an old name left behind.
REPO_ADDRESS = re.compile(r"github\.com/dar1k-ness/rusty-impulse(?![\w-])")

# What `--stale` looks for once the passes have run: the old names, as a tracked file or one of
# its lines has them.
OLD_NAMES = [
    ("ii", r"(?<![A-Za-z0-9])ii(?![A-Za-z0-9])|(?<=\\[nt])ii\b|\bII_[A-Z]|\bii(gen|compiled)|\bIi[A-Z]"),
    ("illogical-impulse", r"(?i)illogical[-_ ]?impulse"),
    ("rusty-impulse", r"(?i)rusty[-_ ]?impulse"),
    (
        "quickshell",
        r"(\.config|\.cache|\.local/state|/tmp|RUNTIME_DIR\}?)/quickshell\b|quickshell:[a-zA-Z$]"
        r"|org[./]quickshell|^\s*import (Quickshell|qs\.)|(?<![.\w])qs(cli|log)\b",
    ),
]
# Where an old name stays: (the files, the lines of them or None for all, why).
ALLOWED = [
    (r"(^|/)LICENSE", None, "a licence's own text"),
    (r"^crates/qml/tests/fixtures/original", None, "the original's files, as the original has them"),
    (r"^crates/rigen/check/cases/", None, "scripts of the original, named after it"),
    (r"^crates/rigen/check/v4\.py$", r"cases/ii_", "scripts of the original, named after it"),
    (r"^tools/parity/(results|audits)/", None, "what an earlier comparison recorded"),
    (r"^crates/qsg/src/(shape|pathsimplify)\.rs$", r"\bii\b", "a variable of the code these follow"),
    (r"^crates/imgtools/src/busy\.rs$", r"`ii\[y, x\]`", "numpy's name in the script this follows"),
    (r"^crates/qml/src/eval\.rs$", r"4\.b\.ii\b", "a step of the ECMAScript specification"),
    (r"", r'alias = "ii"|oldIdentifier: "ii"', "a file written before the rename is still read"),
    (r"", r"ii-qs/|ii\.clsty\.link", "an address of the original's wiki"),
    (r"", r"\(ii wiki\)", "the name of the original's wiki, as the welcome window shows it"),
    (r"^crates/qml/tests/", r"ii shell overrides|Mozilla/5\.0|\bii\(\)", "a test's own data"),
    (r"", r"\bref/ii\b", "where the original is in the history of this repository"),
    (ORIGINAL_COMMAND, r"\b(qs|quickshell) -c ii\b", "the command the original was run with"),
    (r"^tools/install\.sh$", r"local bin=ii tree=ii word=quick", "the build from before the rename, named in halves"),
    (r"^tools/parity/", r'panelFamily.*"ii"|printf %s[-_]%s|i\\\{2\\\}', "what the original reads, in a comparison with it"),
    (r"^bench/", r'"ILLOGICAL" \.\.|printf %s-%s', "what the original reads, in a comparison with it"),
]


# The test of what `tools/carry-over.sh --dots` makes of the commands in a user's files.
CARRY_OVER_DOTS_TEST = (
    "crates/shell/tests/carry_over_dots.rs",
    r'''//! `tools/carry-over.sh --dots new` makes the user's own files name the new build, and
//! `--dots old` puts them back. What is pinned here is that the commands in those files still
//! reach the shell afterwards. The old build answered Quickshell's client, so stock files say
//! `qs -c ii ipc call ...` (or `qs -c $qsConfig ipc call ...`); the new build has a socket of
//! its own that `qs` does not know. (The rewrite made such a line `qs -c rusty-impulse ipc
//! call ...`, which nothing answers.) Each test has a home of its own; the commands of its
//! files are run there with a `qs` and a `rusty-impulse` that write down what they were asked.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A home with the user's files, and the programs their commands call.
struct Home {
    tmp: tempfile::TempDir,
}

/// What the stock files of end-4's dots call the shell with.
const STOCK_CALLS: &str = "\
qs -c $qsConfig ipc call bar toggle
qs -c ii ipc call overview toggle
bash -c 'cliphist store; qs -c $qsConfig ipc call cliphistService update'
qs -p $HOME/.config/quickshell/$qsConfig/welcome.qml
qs -p ~/.config/quickshell/ii/settings.qml
";

impl Home {
    fn new() -> Home {
        let home = Home {
            tmp: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(home.hypr()).unwrap();
        std::fs::create_dir_all(home.path().join("bin")).unwrap();
        // Quickshell's client: it knows the old build's config and no other.
        home.program(
            "bin/qs",
            "case \"$*\" in *rusty-impulse*) echo 'Could not find \"rusty-impulse\" config directory' >&2; exit 255 ;; esac\n\
             echo \"qs $*\" >>\"$HOME/asked\"\n",
        );
        home
    }

    fn path(&self) -> &Path {
        self.tmp.path()
    }

    fn hypr(&self) -> PathBuf {
        self.path().join(".config/hypr/hyprland")
    }

    fn program(&self, name: &str, body: &str) {
        let path = self.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("#!/bin/bash\n{body}")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The new build installed the way the PC has it: `~/.local/bin/rusty-impulse`, a
    /// directory that is not on the PATH of what Hyprland runs.
    fn install_new(&self) {
        self.program(
            ".local/bin/rusty-impulse",
            "echo \"rusty-impulse $*\" >>\"$HOME/asked\"\n",
        );
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.hypr().join(name), text).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.hypr().join(name)).unwrap()
    }

    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command.env_clear().env("HOME", self.path()).env(
            "PATH",
            format!("{}:/usr/bin:/bin", self.path().join("bin").display()),
        );
        command
    }

    fn carry_over(&self, to: &str) {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/carry-over.sh");
        let out = self
            .command("bash")
            .arg(script)
            .args(["--dots", to])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "carry-over.sh --dots {to}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Runs `calls.sh` of the home the way Hyprland runs a binding's command (`qsConfig` is
    /// what `variables.lua` sets) and gives what `qs` and `rusty-impulse` were asked.
    fn asked(&self) -> Vec<String> {
        let _ = std::fs::remove_file(self.path().join("asked"));
        let variables = self.read("variables.lua");
        let config = variables.split('"').nth(3).unwrap();
        let out = self
            .command("bash")
            .arg(self.hypr().join("calls.sh"))
            .env("qsConfig", config)
            .output()
            .unwrap();
        let asked = std::fs::read_to_string(self.path().join("asked")).unwrap_or_default();
        let home = self.path().to_str().unwrap();
        let mut lines: Vec<String> = asked.lines().map(|line| line.replace(home, "~")).collect();
        lines.extend(
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .map(|line| format!("stderr: {line}")),
        );
        lines
    }
}

const VARIABLES: &str = "hl.env(\"qsConfig\", \"ii\")\n";

#[test]
fn stock_calls_through_qs_reach_the_new_build_and_the_old_one_after_going_back() {
    let home = Home::new();
    home.install_new();
    home.write("variables.lua", VARIABLES);
    home.write("calls.sh", STOCK_CALLS);
    let old = [
        "qs -c ii ipc call bar toggle",
        "qs -c ii ipc call overview toggle",
        "qs -c ii ipc call cliphistService update",
        "qs -p ~/.config/quickshell/ii/welcome.qml",
        "qs -p ~/.config/quickshell/ii/settings.qml",
    ];
    assert_eq!(home.asked(), old);

    home.carry_over("new");
    assert_eq!(
        home.asked(),
        [
            "rusty-impulse -c rusty-impulse ipc call bar toggle",
            "rusty-impulse -c rusty-impulse ipc call overview toggle",
            "rusty-impulse -c rusty-impulse ipc call cliphistService update",
            "rusty-impulse -p ~/.config/rustshell/rusty-impulse/welcome.qml",
            "rusty-impulse -p ~/.config/rustshell/rusty-impulse/settings.qml",
        ]
    );
    // By its full path: ~/.local/bin is not on the PATH these commands run with.
    assert!(
        home.read("calls.sh")
            .starts_with("$HOME/.local/bin/rusty-impulse -c $qsConfig ipc call bar toggle\n")
    );

    home.carry_over("old");
    assert_eq!(home.read("calls.sh"), STOCK_CALLS);
    assert_eq!(home.read("variables.lua"), VARIABLES);
    assert_eq!(home.asked(), old);
}

#[test]
fn calls_through_the_old_binary_reach_the_new_one() {
    let home = Home::new();
    home.install_new();
    home.program(".local/bin/ii-rs", "echo \"ii-rs $*\" >>\"$HOME/asked\"\n");
    home.write("variables.lua", VARIABLES);
    // The lines of the PC's keybinds.lua, execs.lua and config.fish.
    let calls = "$HOME/.local/bin/ii-rs -c $qsConfig ipc call sidebarLeft toggle\n\
                 bash -c 'cliphist store && $HOME/.local/bin/ii-rs -c $qsConfig ipc call cliphistService update'\n\
                 ~/.local/bin/ii-rs -c ii ipc call bar toggle\n";
    home.write("calls.sh", calls);
    let old = [
        "ii-rs -c ii ipc call sidebarLeft toggle",
        "ii-rs -c ii ipc call cliphistService update",
        "ii-rs -c ii ipc call bar toggle",
    ];
    assert_eq!(home.asked(), old);

    home.carry_over("new");
    assert_eq!(
        home.asked(),
        [
            "rusty-impulse -c rusty-impulse ipc call sidebarLeft toggle",
            "rusty-impulse -c rusty-impulse ipc call cliphistService update",
            "rusty-impulse -c rusty-impulse ipc call bar toggle",
        ]
    );

    home.carry_over("old");
    assert_eq!(home.read("calls.sh"), calls);
    assert_eq!(home.asked(), old);
}

#[test]
fn a_build_that_is_not_under_the_home_is_called_by_name() {
    let home = Home::new();
    home.program(
        "bin/rusty-impulse",
        "echo \"rusty-impulse $*\" >>\"$HOME/asked\"\n",
    );
    home.write("variables.lua", VARIABLES);
    home.write("calls.sh", "qs -c ii ipc call bar toggle\n");
    home.carry_over("new");
    assert_eq!(
        home.read("calls.sh"),
        "rusty-impulse -c rusty-impulse ipc call bar toggle\n"
    );
    assert_eq!(
        home.asked(),
        ["rusty-impulse -c rusty-impulse ipc call bar toggle"]
    );
}

/// The stock fish alias `q` is typed as `q ipc call ...`.
#[test]
fn an_alias_for_the_old_client_calls_the_new_build() {
    let home = Home::new();
    home.install_new();
    home.write("variables.lua", VARIABLES);
    let fish = home.path().join(".config/fish/config.fish");
    std::fs::create_dir_all(fish.parent().unwrap()).unwrap();
    std::fs::write(&fish, "    alias q 'qs -c ii'\n").unwrap();
    home.carry_over("new");
    let alias = std::fs::read_to_string(&fish).unwrap();
    assert_eq!(
        alias,
        "    alias q '$HOME/.local/bin/rusty-impulse -c rusty-impulse'\n"
    );
    let body = alias.split('\'').nth(1).unwrap();
    home.write("calls.sh", &format!("{body} ipc call bar toggle\n"));
    assert_eq!(
        home.asked(),
        ["rusty-impulse -c rusty-impulse ipc call bar toggle"]
    );
    home.carry_over("old");
    assert_eq!(
        std::fs::read_to_string(&fish).unwrap(),
        "    alias q 'qs -c ii'\n"
    );
}

#[test]
fn what_is_quickshells_own_stays() {
    let home = Home::new();
    home.install_new();
    // The original started by its config, a tree that is not this shell's, the processes a
    // restart binding ends, and a program whose name only ends in qs.
    let stays = "qs -c ii\n\
                 hl.exec_cmd(\"qs -c ii\")\n\
                 qs -p ~/src/other/shell.qml\n\
                 qs -c caelestia ipc call drawers toggle\n\
                 killall ydotool qs quickshell\n\
                 pidof qs quickshell hyprlock\n\
                 /usr/bin/qs --version\n";
    home.write(
        "keybinds.lua",
        &format!("{stays}qs -c ii ipc call bar toggle\n"),
    );
    home.carry_over("new");
    assert_eq!(
        home.read("keybinds.lua"),
        format!("{stays}$HOME/.local/bin/rusty-impulse -c rusty-impulse ipc call bar toggle\n")
    );
}
''',
)

OLD_NAMES_TEST = (
    "crates/shell/tests/old_names.rs",
    '''//! The names the project had before it was renamed are in no tracked file, outside the places
//! `tools/rename/rename.py` lists with the reason for each (`ALLOWED`, and the files it leaves
//! as they are): `tools/rename/rename.py --stale` finds none.
use std::path::Path;
use std::process::Command;

#[test]
fn no_old_name_is_left() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let o = Command::new("python3")
        .arg("tools/rename/rename.py")
        .arg("--stale")
        .current_dir(&root)
        // The checkout may be another user's (a container that mounts it).
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "safe.directory")
        .env("GIT_CONFIG_VALUE_0", "*")
        .output()
        .expect("python3");
    assert!(
        o.status.success(),
        "old names are left:\\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}
''',
)


def stale():
    """Lists what is left of the old names outside SKIP and ALLOWED; 1 if there is any."""
    old = [(k, re.compile(rx)) for k, rx in OLD_NAMES]
    allowed = [(re.compile(f), re.compile(l) if l else None) for f, l, _ in ALLOWED]
    found = 0
    for rel in tracked():
        if rel.startswith(SKIP4):
            continue
        here = [l for f, l in allowed if f.search(rel)]
        # The name pass 4 replaced is looked for in the files the other passes leave alone too.
        first = rel.startswith(SKIP) or None in here
        for k, rx in old:
            if first and k != "rusty-impulse":
                continue
            if rx.search(rel):
                found += 1
                print(f"{rel}: the path has {k}")
        try:
            with open(os.path.join(ROOT, rel), encoding="utf-8") as f:
                lines = f.read().split("\n")
        except (FileNotFoundError, IsADirectoryError, UnicodeDecodeError):
            continue
        for n, line in enumerate(lines, 1):
            kept = CREDIT.search(line) or any(l and l.search(line) for l in here)
            line = REPO_ADDRESS.sub("", line)
            for k, rx in old:
                if k != "rusty-impulse" and (first or kept):
                    continue
                if k == "quickshell" and rel.startswith(SKIP2):
                    continue  # written in real Quickshell's words
                if rx.search(line):
                    found += 1
                    print(f"{rel}:{n}: {k}: {line.strip()[:160]}")
    print(f"stale: {found} old names left", file=sys.stderr)
    return 1 if found else 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--stale", action="store_true", help="list what is left of the old names")
    ap.add_argument("--pass", dest="stage", type=int, default=1, choices=(1, 2, 3, 4))
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args()
    if a.stale:
        sys.exit(stale())
    content = [(re.compile(r[0]), r[1], re.compile(r[2]) if len(r) > 2 else None) for r in rules(a.stage)]
    always = [(re.compile(p), r, re.compile(r"^(tools/parity|bench)/")) for p, r in ALWAYS2]
    renames = [(re.compile(p), r) for p, r in paths(a.stage)]

    edited = moved = 0
    for rel in tracked():
        if rel.startswith(SKIP4 if a.stage == 4 else SKIP) or (a.stage == 3 and rel.startswith(SKIP3)):
            continue
        rules_here = content
        if a.stage == 2 and rel.startswith(SKIP2):
            rules_here = always  # written in real Quickshell's words: only what is surely ours
        full = os.path.join(ROOT, rel)
        try:
            with open(full, "rb") as f:
                raw = f.read()
        except FileNotFoundError:
            continue
        if b"\0" in raw[:8192]:
            continue  # a binary file keeps its bytes
        try:
            text = raw.decode()
        except UnicodeDecodeError:
            continue
        lines = text.split("\n")
        for i, line in enumerate(lines):
            if a.stage != 4 and CREDIT.search(line):
                continue
            new = line
            for rx, rep, only in rules_here:
                if only is None or only.search(rel):
                    new = rx.sub(rep, new)
            lines[i] = new
        new_text = "\n".join(lines)
        if a.stage == 3:
            right = prose.put_right(rel, new_text).split("\n")
            assert len(right) == len(lines), rel
            for i, line in enumerate(lines):
                if CREDIT.search(line):
                    right[i] = line
            new_text = "\n".join(right)
        if new_text != text:
            edited += 1
            if not a.check:
                with open(full, "w") as f:
                    f.write(new_text)

    for rel in tracked():
        if rel.startswith(SKIP2 if a.stage == 2 else KEEP_PATH4 if a.stage == 4 else KEEP_PATH):
            continue
        new = rel
        for rx, rep in renames:
            new = rx.sub(rep, new)
        if new != rel:
            moved += 1
            if not a.check:
                os.makedirs(os.path.dirname(os.path.join(ROOT, new)), exist_ok=True)
                subprocess.run(["git", "-C", ROOT, "mv", rel, new], check=True)
                # the directory the file left, once it is empty
                was = os.path.dirname(os.path.join(ROOT, rel))
                while was != ROOT and not os.listdir(was):
                    os.rmdir(was)
                    was = os.path.dirname(was)
    for rel, was, now in EDITS.get(a.stage, []):
        full = os.path.join(ROOT, rel)
        if not os.path.exists(full) and os.path.exists(os.path.join(ROOT, renamed4_path(rel))):
            full = os.path.join(ROOT, renamed4_path(rel))  # the tree is renamed a second time (pass 4)
        if rel == PC_DEFAULT and not os.path.exists(full):
            continue
        with open(full) as f:
            text = f.read()
        if now in text or renamed4(now, rel) in text:
            continue
        if was not in text:
            sys.exit(f"{rel}: the text of a hand edit is not there:\n{was}")
        edited += 1
        if not a.check:
            with open(full, "w") as f:
                f.write(text.replace(was, now, 1))
    if a.stage == 1 and not a.check:
        after_pass_1()
    if a.stage == 3:
        # the test that keeps the old names out from here on
        for rel, text in (OLD_NAMES_TEST, CARRY_OVER_DOTS_TEST):
            test = os.path.join(ROOT, rel)
            if not os.path.exists(test):
                edited += 1
                if not a.check:
                    with open(test, "w") as f:
                        f.write(text)
                    subprocess.run(["git", "-C", ROOT, "add", rel], check=True)
    if a.stage in (3, 4) and not a.check:
        subprocess.run(["cargo", "fmt", "--all"], cwd=ROOT, check=True)
    if a.stage == 2 and not a.check:
        subprocess.run(["cargo", "fmt", "--all"], cwd=ROOT, check=True)
        # the table of unwritten methods is in the order of its lines, which have new names
        table = os.path.join(ROOT, "docs/unwritten-methods.tsv")
        with open(table) as f:
            head, *rows = f.read().splitlines(keepends=True)
        with open(table, "w") as f:
            f.write(head + "".join(sorted(rows)))
    print(f"pass {a.stage}: {edited} files edited, {moved} paths renamed", file=sys.stderr)

if __name__ == "__main__":
    main()
