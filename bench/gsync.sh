#!/bin/bash
# Sync impulse-shell to the Arch guest and build there (system freetype/harfbuzz match the user's Arch).
cd ~/work/impulse-shell && tar czf /tmp/impulse-shell.tgz --exclude=target --exclude=.git . && \
scp -q -P 2222 -i ~/work/vm/id -o StrictHostKeyChecking=no /tmp/impulse-shell.tgz dar1k@127.0.0.1:/tmp/ && \
~/work/vm/ssh 'mkdir -p ~/impulse-shell && cd ~/impulse-shell && tar xzf /tmp/impulse-shell.tgz && cargo build --release --workspace --features shell/interp 2>&1 | grep -E "^(error|warning)" -A7 | head -40; ls -la target/release/textscene 2>&1 | tail -1'
