#!/usr/bin/env bash
# Regenerates fixtures/textcolor/regions.txt with ScreenTextOverlay's real pipeline
# (`magick F +repage -crop G png:- | text-color-venv.sh`) on a machine with the original installed.
# Usage: textcolor_region_golden.sh FIXTURE_DIR RI_SCRIPTS_DIR > regions.txt
set -u
cd "$1"
for f in *.png *.jpg; do
    for g in 5x5+0+0 50x30+10+4 50x50+190+40 1x1+0+0 0x0+3+2 0x7+1+1 30x0+2+0 \
        5x5+300+0 5x5+0+300 47.6x20.2+10.5+10.4 3.5x2.5+0.5+1.5 12.49x9.51+2.51+1.49; do
        printf '%s %s %s\n' "$f" "$g" "$(magick "$f" +repage -crop "$g" png:- 2>/dev/null | "$2/images/text-color-venv.sh")"
    done
done
