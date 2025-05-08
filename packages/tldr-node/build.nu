#!/usr/bin/env nu

cd

tldr --update

'{
    "skipUpdateWhenPageNotFound": true
}
' | save -f .tldrrc

mkdir .local/share/bash-completion/completions/
ln -frs opt/npm/tldr/node_modules/tldr/bin/completion/bash/tldr .local/share/bash-completion/completions/

mkdir .zfunc
ln -frs opt/npm/tldr/node_modules/tldr/bin/completion/zsh/_tldr .zfunc/

tar --create --file provides.tar --verbatim-files-from --files-from w/provides.txt
