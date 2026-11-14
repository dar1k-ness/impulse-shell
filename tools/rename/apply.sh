#!/bin/bash
# apply.sh [CARRY]: the whole rename on the checkout this is in, as commits on its branch.
#
#   git checkout -b <new branch> <the head to rename>
#   git checkout <a branch that has tools/rename> -- tools/rename
#   tools/rename/apply.sh <head of the carry-over branch>
#
# CARRY is the commit the carry-over of the owner's data is taken from (tools/carry-over.sh,
# tools/pc, tools/install.sh and its test, written for the new names); without it those files
# are only renamed. Each pass is run twice, and all three once more at the end: a pass that
# still finds something to change stops this. Then `rename.py --stale` must find no old name.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
carry=${1:-}
[ -z "$(git status --porcelain -- . ':!tools/rename')" ] || { echo "apply.sh: the checkout has changes of its own" >&2; exit 1; }
pass() { python3 tools/rename/rename.py --pass "$1" 2>&1 | tail -n 1; }
settled() {
    local said
    said=$(python3 tools/rename/rename.py --pass "$1" --check 2>&1 | tail -n 1)
    [ "$said" = "pass $1: 0 files edited, 0 paths renamed" ] || { echo "apply.sh: pass $1 is not settled: $said" >&2; exit 1; }
}
commit() { git add -A && git commit -q -m "$1"; }

pass 1
commit "Rename illogical-impulse and ii to rusty-impulse (tools/rename/rename.py --pass 1)"
if [ -n "$carry" ]; then
    git checkout -q "$carry" -- tools/carry-over.sh tools/pc tools/install.sh crates/shell/tests/install_rollback.rs
    pass 1
    # The variable the carry-over reads is listed with the others (docs/env.md).
    python3 - <<'PY'
import re
doc = "docs/env.md"
text = open(doc).read()
if "RI_WORD" in open("tools/carry-over.sh").read() and "`RI_WORD`" not in text:
    row = (
        "| `RI_WORD` | `tools/carry-over.sh`, `tools/pc/rollback-watch` | The word that stands in place of"
        " `quickshell` in the project's own names (default `rustshell`). `tools/carry-over.sh` names with it"
        " the directories it carries a user's data over to, and stops if it is `quickshell` or not a name;"
        " `tools/pc/rollback-watch` takes it as the prefix of the new build's layers and the name of its"
        " runtime directory. Set it only to the word the installed build was renamed with. |\n"
    )
    at = re.search(r"^\| `RI_ROOT_SUM` .*\n", text, re.M)
    assert at, "docs/env.md has no RI_ROOT_SUM row to put RI_WORD after"
    open(doc, "w").write(text[: at.end()] + row + text[at.end() :])
PY
    commit "The carry-over of a user's data, the release tree and the PC scripts, as of $(git rev-parse "$carry")"
fi
settled 1
pass 2
settled 2
commit "Rename the engine's own quickshell names to rustshell (tools/rename/rename.py --pass 2)"
pass 3
settled 3
commit "Call the original the original where ii was a word of its own (tools/rename/rename.py --pass 3)"
for p in 1 2 3; do settled "$p"; done
python3 tools/rename/rename.py --stale
git log --oneline -n 4
