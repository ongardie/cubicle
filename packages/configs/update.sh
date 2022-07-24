#!/bin/sh
set -eu
cp -a dot-profile ~/.profile
ln -fs .profile ~/.zprofile
cp -a dot-zshrc ~/.zshrc
touch ~/.UPDATED
