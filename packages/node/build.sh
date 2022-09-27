#!/bin/sh
set -eu

asdf plugin list | grep '^nodejs$' || asdf plugin add nodejs

asdf install nodejs latest

mkdir -p ~/.dev-init/
echo 'asdf global nodejs latest' > ~/.dev-init/node-asdf.sh
chmod +x ~/.dev-init/node-asdf.sh

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
