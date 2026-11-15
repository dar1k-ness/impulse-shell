#!/bin/bash
# apply4.sh [PICK...]: the second rename, rusty-impulse to impulse-shell (rename.py --pass 4), on
# the checkout this is in, as commits on its branch.
#
#   git checkout -b <new branch> <the head to rename>
#   git cherry-pick <the commit that added pass 4 and this file>
#   tools/rename/apply4.sh <first>..<last>
#
# PICK names commits written on a tree that is renamed already (a commit, or a range A..B): what
# carries a user's data over from the build under the previous name, and the PC scripts that go
# back to that build. They are picked in this order after the pass; one that does not apply
# stops this with the conflict in the working tree (mend it, `git cherry-pick --continue`, then
# run the checks at the end of this file by hand).
# The pass must find nothing more to change once it has run, and neither must passes 1 to 3;
# then `rename.py --stale` must find no old name.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
[ -z "$(git status --porcelain -- . ':!tools/rename')" ] || { echo "apply4.sh: the checkout has changes of its own" >&2; exit 1; }
settled() {
    local said
    said=$(python3 tools/rename/rename.py --pass "$1" --check 2>&1 | tail -n 1)
    [ "$said" = "pass $1: 0 files edited, 0 paths renamed" ] || { echo "apply4.sh: pass $1 is not settled: $said" >&2; exit 1; }
}

python3 tools/rename/rename.py --pass 4 2>&1 | tail -n 1
settled 4
git add -A
git commit -q -m "Rename rusty-impulse to impulse-shell (tools/rename/rename.py --pass 4)"
for pick in "$@"; do git cherry-pick "$pick"; done
for p in 1 2 3 4; do settled "$p"; done
python3 tools/rename/rename.py --stale
git log --oneline -n $(($# + 4))
