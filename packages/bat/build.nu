#!/usr/bin/env nu

cd

mkdir .local/share/bash-completion/completions
bat --completion bash | save -f .local/share/bash-completion/completions/bat

mkdir .zfunc
bat --completion zsh | save -f .zfunc/_bat

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
