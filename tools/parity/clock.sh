#!/bin/bash
# Keep the wall clock inside 2026-01-15 12:34:30..31 so clock/date texts match between runs.
while :; do date -s "2026-01-15 12:34:30" >/dev/null; sleep 0.5; done
