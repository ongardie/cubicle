#!/bin/sh
set -eu

asdf plugin list | grep '^golang$' || asdf plugin add golang

asdf install golang latest

mkdir -p ~/.dev-init/
echo 'asdf global golang latest' > ~/.dev-init/go-asdf.sh
chmod +x ~/.dev-init/go-asdf.sh

asdf global golang latest
install="$(asdf where golang)/packages/bin"
install="$HOME${install#$HOME}"
echo "$install" > ~/.configs/profile.d/path/36-go

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
