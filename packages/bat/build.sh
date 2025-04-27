#!/bin/sh

set -eux
cd

mkdir -p .local/share/bash-completion/completions
bat --completion bash >> .local/share/bash-completion/completions/bat

mkdir -p .zfunc
bat --completion zsh >> .zfunc/_bat

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
