#!/bin/sh
set -eu

cd

tldr --update

cat > .tldrrc <<'EOF'
{
    "skipUpdateWhenPageNotFound": true
}
EOF

mkdir -p .local/share/bash-completion/completions/
ln -frs opt/npm/tldr/node_modules/tldr/bin/completion/bash/tldr .local/share/bash-completion/completions/

mkdir -p .zfunc
ln -frs opt/npm/tldr/node_modules/tldr/bin/completion/zsh/_tldr .zfunc/

tar --create --file provides.tar --verbatim-files-from --files-from w/provides.txt
