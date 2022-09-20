#!/bin/sh
set -eu

# Note: this assumption is unlikely to hold for many packages.
BIN="$PACKAGE"

cd
shiv --console-script "$BIN" --output-file "bin/$BIN" "$PACKAGE"
tar --create --file provides.tar "bin/$BIN"
