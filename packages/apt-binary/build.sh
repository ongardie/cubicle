#!/bin/sh
set -eu

cp -a bin/* ~/bin/
cp -a bashrc.d/* ~/.config/bashrc.d/
cp -a zshrc.d/* ~/.config/zshrc.d/

mkdir -p ~/.config/nushell/autoload/
cp -a nushell/* ~/.config/nushell/autoload/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
