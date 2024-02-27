#!/bin/sh

set -eu

mkdir -p ~/.vscode-oss/extensions/
cd ~/.vscode-oss/extensions/

if [ -n "$(find . -maxdepth 1 -name '*.json' -not -name extensions.json)" ]; then
    jq --slurp 'map(.[0])' ./*.json > extensions.json.merged
    rm ./*.json
    mv extensions.json.merged extensions.json
fi
