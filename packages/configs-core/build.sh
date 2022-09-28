#!/bin/sh
set -eu

mkdir -p ~/.configs/

ln -fs .bashrc ~/.bash_profile
cp -a dot-bashrc ~/.bashrc
cp -a bashrc.d/ ~/.configs/bashrc.d/
cp -a profile.d/ ~/.configs/profile.d/
cp -a zshrc.d/ ~/.configs/zshrc.d/
cp -a dot-profile ~/.profile
ln -fs .profile ~/.zprofile
cp -a dot-zshrc ~/.zshrc

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
