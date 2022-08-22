#!/bin/sh
set -eu
cp -a dot-profile ~/.profile
ln -fs .profile ~/.zprofile
cp -a dot-zshrc ~/.zshrc
tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
