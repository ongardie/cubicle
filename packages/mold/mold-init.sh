#!/bin/sh

mkdir -p ~/.cargo
if ! grep -q mold ~/.cargo/config.toml 2>/dev/null; then
    cat >> ~/.cargo/config.toml << EOF
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=$HOME/bin/mold"]
EOF
fi
