#!/bin/sh
set -eu
cd
codium --install-extension "$PACKAGE" --force
tar --create --file provides.tar .vscode-oss/extensions
