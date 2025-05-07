#!/bin/sh
set -eu

mkdir -p ~/.config/bashrc.d/
cp -a bashrc.d/* ~/.config/bashrc.d/

mkdir -p ~/.config/nushell/autoload/
cp -a nushell/* ~/.config/nushell/autoload/

mkdir -p ~/.config/shrc.d/
cp -a shrc.d/* ~/.config/shrc.d/

mkdir -p ~/.config/zshrc.d/
cp -a zshrc.d/* ~/.config/zshrc.d/

mkdir -p ~/.dev-init/
cp -a nushell-history.sh ~/.dev-init/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
