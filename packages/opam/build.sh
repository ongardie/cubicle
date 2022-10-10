#!/bin/sh
set -eux

if ! [ -f ~/bin/opam ]; then
    if ! [ -f install.sh ]; then
        curl -LO 'https://raw.githubusercontent.com/ocaml/opam/master/shell/install.sh'
        chmod +x install.sh
    fi

    ./install.sh --download-only
    mv opam-* ~/bin/opam
    chmod +x ~/bin/opam
fi

opam init --disable-sandboxing --no-setup --reinit

cp profile.sh ~/.config/profile.d/70-opam.sh

mkdir -p ~/.local/share/bash-completion/completions
ln -frs ~/.opam/opam-init/complete.sh ~/.local/share/bash-completion/completions/opam

mkdir -p ~/.zfunc
ln -frs ~/.opam/opam-init/complete.zsh ~/.zfunc/_opam

echo '$HOME/.opam/default/bin' > ~/.config/profile.d/path/37-opam

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
