#!/usr/bin/env nu

cd

# Default name and email from git at container init.
mkdir .dev-init
cp w/jj-user-from-git.sh .dev-init/
chmod +x .dev-init/jj-user-from-git.sh

# Bash completion.

mkdir .config/bashrc.d
$"#!/bin/bash

(env COMPLETE=bash jj)
" | save -f .config/bashrc.d/80-jj.bash
chmod +x .config/bashrc.d/80-jj.bash

# Nushell completion.

mkdir .config/nushell/autoload
jj util completion nushell | save -f .config/nushell/autoload/80-jj.nu


# Zsh completion.

mkdir .config/zshrc.d
$"#!/bin/zsh

(env COMPLETE=zsh jj)
" | save -f .config/zshrc.d/80-jj.zsh
chmod +x .config/zshrc.d/80-jj.zsh

# Man pages.
rm -rf .local/share/man
mkdir .local/share/man
jj util install-man-pages .local/share/man

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
