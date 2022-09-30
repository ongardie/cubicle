#!/bin/sh
set -eu

mkdir -p ~/.config/

ln -fs .bashrc ~/.bash_profile
cp -a dot-bashrc ~/.bashrc
cp -a bashrc.d/ ~/.config/bashrc.d/
cp -a profile.d/ ~/.config/profile.d/
cp -a zshrc.d/ ~/.config/zshrc.d/
cp -a dot-profile ~/.profile
ln -fs .profile ~/.zprofile
cp -a dot-zshrc ~/.zshrc

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
