#!/bin/sh
set -eu

mkdir -p ~/.config/

cp -a dot-bash_profile ~/.bash_profile
cp -a dot-bashrc ~/.bashrc
cp -a bashrc.d/ ~/.config/bashrc.d/
cp -a profile.d/ ~/.config/profile.d/
cp -a shrc.d/ ~/.config/shrc.d/
cp -a zshrc.d/ ~/.config/zshrc.d/
cp -a dot-profile ~/.profile
cp -a dot-zprofile ~/.zprofile
cp -a dot-zshrc ~/.zshrc

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
