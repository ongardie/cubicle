#!/bin/sh
set -eu

cp -a bashrc.d/* ~/.config/bashrc.d/
cp -a shrc.d/* ~/.config/shrc.d/
cp -a zshrc.d/* ~/.config/zshrc.d/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
