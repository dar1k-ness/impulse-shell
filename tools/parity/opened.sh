#!/bin/sh
# The guest's handler for folders (setup.sh registers it): records what the shell asked the
# desktop to open, since the guest has no file manager to show it.
echo "$1" >> /tmp/parity-opened.log
