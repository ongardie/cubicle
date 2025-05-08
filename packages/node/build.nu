#!/usr/bin/env nu

if 'nodejs' not-in (^asdf plugin list | lines) {
    asdf plugin add nodejs
}

asdf install nodejs latest

mkdir ~/.dev-init/
'
#!/bin/sh
asdf set --home nodejs latest
' | save -f ~/.dev-init/node-asdf.sh
chmod +x ~/.dev-init/node-asdf.sh

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
