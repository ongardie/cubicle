#!/bin/sh
set -ex

mkdir -p ~/opt
cd ~/opt

if [ -d mold ]; then
    cd mold
    git fetch --all
else
    git clone https://github.com/rui314/mold.git
    cd mold
fi

TAG=$(git tag | grep -E 'v1.[0-9]+.[0-9]+' | sort --version-sort | tail -n 1)
git checkout $TAG

make -j$(nproc)
cp mold ~/bin/mold
mold --version

mkdir -p ~/.cargo
if ! grep -q mold ~/.cargo/config.toml; then
    cat >> ~/.cargo/config.toml << EOF
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=$HOME/bin/mold"]
EOF
fi

tar -c -C ~ --verbatim-files-from --files-from ~/$SANDBOX/provides.txt -f ~/provides.tar
