#!/bin/bash
# Sync rusty-impulse to the Arch guest and build there (system freetype/harfbuzz match the user's Arch).
cd ~/work/rusty-impulse && tar czf /tmp/rusty-impulse.tgz --exclude=target --exclude=.git . && \
scp -q -P 2222 -i ~/work/vm/id -o StrictHostKeyChecking=no /tmp/rusty-impulse.tgz dar1k@127.0.0.1:/tmp/ && \
~/work/vm/ssh 'mkdir -p ~/rusty-impulse && cd ~/rusty-impulse && tar xzf /tmp/rusty-impulse.tgz && cargo build --release --workspace --features shell/interp 2>&1 | grep -E "^(error|warning)" -A7 | head -40; ls -la target/release/textscene 2>&1 | tail -1'
