#!/bin/sh
set -e

echo "Checking latest version of VS Codium"
RELEASES=$TMPDIR/vscodium-releases
curl -sS 'https://api.github.com/repos/VSCodium/vscodium/releases' > $RELEASES
latest=$(jq -r '.[] | .tag_name' < $RELEASES | sort --version-sort | tail -n 1)
if [ v"$( ~/opt/vscodium/bin/codium --version | head -n 1 )" = v"$latest" ]; then
    echo "Have latest VS Codium already ($latest)"
else
    rm -rf ~/opt/vscodium
    mkdir -p ~/opt/vscodium
    cd ~/opt/vscodium
    echo "Downloading VSCodium $latest"
    curl -LS "https://github.com/VSCodium/vscodium/releases/download/$latest/VSCodium-linux-x64-$latest.tar.gz" | tar -xz
fi
ln -fs ~/opt/vscodium/bin/codium ~/bin/codium


mkdir -p ~/opt/vscodium-extensions
cd ~/opt/vscodium-extensions
for id in \
    dbaeumer.vscode-eslint \
    esbenp.prettier-vscode \
    golang.go \
    lextudio.restructuredtext \
    llvm-vs-code-extensions.vscode-clangd \
    matklad.rust-analyzer \
    ms-python.python \
    stkb.rewrap \
    streetsidesoftware.code-spell-checker \
    tamasfe.even-better-toml \
    usernamehw.errorlens \
    vscodevim.vim \
; do
    echo "Checking latest version of $id"
    download=$(curl -LsS "https://open-vsx.org/api/-/query?extensionId=$id" | jq -r '.extensions[0].files.download')
    if [ "$download" = "null" ]; then
        echo "Extension $id not found"
        exit 1
    fi
    file=$(basename "$download")
    if [ ! -f $file ]; then
        echo "Downloading $file"
        rm -f "$id-*.vsix"
        curl -LOsS "$download"
    fi
    if [ ! -d ~/.vscode-oss/extensions/$(basename "$file" .vsix | tr '[:upper:]' '[:lower:]') ]; then
        echo "Installing $file"
        codium --install-extension "$file"
    fi
done

touch ~/.UPDATED
