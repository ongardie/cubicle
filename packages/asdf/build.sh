#!/bin/sh
set -eux

echo "Checking latest version of asdf on GitHub"
RELEASES="$HOME/w/asdf-releases.json"
curl -sS 'https://api.github.com/repos/asdf-vm/asdf/releases' -o "$RELEASES"

machine=$(uname -m)
case "$machine" in
    aarch64)
        arch=arm64
        ;;
    x86_64)
        arch=amd64
        ;;
    *)
        echo "Architecture not supported: uname -m: $machine"
        exit 1
        ;;
esac

download=$(jq -r 'map(select(.prerelease == false)) | .[0].assets | map(.browser_download_url | select(test("/asdf-v.*-linux-'"$arch"'.tar.gz$")))[0]' "$RELEASES")
tarball=$(basename "$download")

if [ -z "$download" ] || [ "$download" = "null" ]; then
    exit 1
fi

cd "$TMPDIR"
if ! [ -f "$tarball" ]; then
    curl -LO "$download"
fi

tar -C "$HOME/bin" -xvf "$tarball"

# shellcheck disable=SC2016
echo '$HOME/.asdf/shims' > ~/.config/profile.d/path/50-asdf

mkdir -p ~/.local/share/bash-completion/completions/
asdf completion bash > ~/.local/share/bash-completion/completions/asdf

mkdir -p ~/.zfunc
asdf completion zsh > ~/.zfunc/_asdf

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
