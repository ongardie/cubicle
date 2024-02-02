#!/bin/sh
set -eu

if [ -d ~/.asdf ]; then
    cd ~/.asdf
    git fetch --all
else
    git clone https://github.com/asdf-vm/asdf.git ~/.asdf
    cd ~/.asdf
fi

RELEASES="$HOME/w/asdf-releases"
curl -sS 'https://api.github.com/repos/asdf-vm/asdf/releases' -o "$RELEASES"
version=$(cat "$RELEASES" | jq -r 'map(select(.prerelease == false)) | .[0].tag_name')
git reset --hard "$version"

cat > ~/.config/profile.d/90-asdf.sh <<"EOF"
export ASDF_DIR="$HOME/.asdf"
. $HOME/.asdf/asdf.sh
EOF

mkdir -p ~/.local/share/bash-completion/completions/
ln -frs ~/.asdf/completions/asdf.bash ~/.local/share/bash-completion/completions/asdf

mkdir -p ~/.zfunc
ln -frs ~/.asdf/completions/_asdf ~/.zfunc/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
