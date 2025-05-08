#!/usr/bin/env nu

cd

mkdir .local/share/bash-completion/completions
just --completions bash | save -f .local/share/bash-completion/completions/just

mkdir .config/nushell/autoload/
just --completions nushell | save -f .config/nushell/autoload/80-just.nu

mkdir .zfunc
just --completions zsh | save -f .zfunc/_just

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
