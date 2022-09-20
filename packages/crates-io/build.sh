#!/bin/sh
set -eu
cd
if ! [ -e exclude.txt ]; then
    find .cargo/bin/* > exclude.txt
fi
cargo install --force $PACKAGE
tar --create --file provides.tar --exclude-from exclude.txt .cargo/bin
