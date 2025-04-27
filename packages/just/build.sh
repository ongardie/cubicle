#!/bin/sh

set -eux
cd

mkdir -p .local/share/bash-completion/completions
just --completions bash >> .local/share/bash-completion/completions/just

mkdir -p .config/nushell/autoload/
just --completions nushell >> .config/nushell/autoload/80-just.nu

mkdir -p .zfunc
just --completions zsh >> .zfunc/_just

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
