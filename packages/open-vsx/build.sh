#!/bin/sh
set -eu
cd
codium --install-extension "$PACKAGE" --force
cp -a .vscode-oss/extensions/extensions.json ".vscode-oss/extensions/$PACKAGE.json"
tar --create --file provides.tar .vscode-oss/extensions
