#!/bin/sh
set -eu
cd
if ! [ -e exclude.txt ]; then
    find go/bin/* > exclude.txt
fi
go install "$PACKAGE"@latest
tar --create --file ~/provides.tar --exclude-from exclude.txt go/bin
