#!/usr/bin/env nu

if 'golang' not-in (^asdf plugin list | lines) {
    asdf plugin add golang
}

asdf install golang latest

mkdir ~/.dev-init/
'asdf set --home golang latest' | save -f ~/.dev-init/go-asdf.sh
chmod +x ~/.dev-init/go-asdf.sh

asdf set --home golang latest

[
    '$HOME'
    (asdf where golang | path relative-to ~)
    bin
] | path join | save -f ~/.config/profile.d/path/36-go

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
