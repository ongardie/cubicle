#!/usr/bin/env nu

if ("~/bin/opam" | path exists) == false {
    if ("install.sh" | path exists) == false {
        http get 'https://raw.githubusercontent.com/ocaml/opam/master/shell/install.sh'
            | save -f install.sh
        chmod +x install.sh
    }

    ./install.sh --download-only
    mv opam-* ~/bin/opam
    chmod +x ~/bin/opam
}

opam init --disable-sandboxing --no-setup --reinit

cp profile.sh ~/.config/profile.d/70-opam.sh
cp profile.nu ~/.config/nushell/autoload/70-opam.nu

mkdir ~/.local/share/bash-completion/completions
ln -frs ~/.opam/opam-init/complete.sh ~/.local/share/bash-completion/completions/opam

mkdir ~/.zfunc
ln -frs ~/.opam/opam-init/complete.zsh ~/.zfunc/_opam

echo '$HOME/.opam/default/bin' | save -f ~/.config/profile.d/path/37-opam

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
