#!/bin/sh
set -eu
cd

if ! [ -e exclude.txt ]; then
    find .cargo/bin/* > exclude.txt
fi
cargo install --force "$PACKAGE"

# shellcheck disable=SC2016
echo '$HOME/.cargo/bin' > .config/profile.d/path/33-cargo

tar --create --file provides.tar --exclude-from exclude.txt .cargo/bin .config/profile.d/path/33-cargo
