#!/bin/sh

set -eu

cd "$TMPDIR"
arch="$(uname -m)"
curl -LO "https://github.com/cargo-bins/cargo-binstall/releases/latest/download/cargo-binstall-$arch-unknown-linux-musl.tgz"
tar -xf "cargo-binstall-$arch-unknown-linux-musl.tgz"
mkdir -p ~/.cargo/bin
mv cargo-binstall ~/.cargo/bin

cd
mkdir -p .config/profile.d
cp w/50-cargo-binstall.sh .config/profile.d/50-cargo-binstall.sh

tar --create --file provides.tar .cargo/bin/cargo-binstall .config/profile.d/50-cargo-binstall.sh
