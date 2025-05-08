#!/usr/bin/env nu

cd
codium --install-extension $env.PACKAGE --force
cp .vscode-oss/extensions/extensions.json $".vscode-oss/extensions/($env.PACKAGE).json"
tar --create --file provides.tar .vscode-oss/extensions
