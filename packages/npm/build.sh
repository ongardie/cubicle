#!/bin/sh
set -eu

cd
mkdir -p "opt/npm/$PACKAGE"
cd "opt/npm/$PACKAGE"
npm install --global-style "$PACKAGE"
cd node_modules/.bin
set -- "opt/npm/$PACKAGE"
for f in *; do
    echo "Found executable: $f"
    ln -fs "../opt/npm/$PACKAGE/node_modules/.bin/$f" ~/bin/
    set -- "$@" "bin/$f"
done

cd
tar --create --file provides.tar "$@"
