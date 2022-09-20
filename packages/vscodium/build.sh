#!/bin/sh
set -e

curl() {
    command curl --connect-timeout 10 --location --max-time 20 --show-error "$@"
}

echo "Checking latest version of VS Codium"
RELEASES=$TMPDIR/vscodium-releases
curl -s 'https://api.github.com/repos/VSCodium/vscodium/releases' > $RELEASES
latest=$(jq -r '.[] | .tag_name' < $RELEASES | sort --version-sort | tail -n 1)
if [ "$( ~/opt/vscodium/bin/codium --version | head -n 1 )" = "$latest" ]; then
    echo "Have latest VS Codium already ($latest)"
else
    rm -rf ~/opt/vscodium
    mkdir -p ~/opt/vscodium
    cd ~/opt/vscodium
    echo "Downloading VSCodium $latest"
    curl --max-time 120 "https://github.com/VSCodium/vscodium/releases/download/$latest/VSCodium-linux-x64-$latest.tar.gz" | tar -xz
fi
ln -fs ../opt/vscodium/bin/codium ~/bin/codium

mkdir -p ~/opt/vscodium-extensions
cd ~/opt/vscodium-extensions

# Get the rust-analyzer extension from GitHub releases, since the version on
# OpenVSX is stale and somewhat broken. See
# <https://github.com/rust-lang/rust-analyzer/issues/11080>.
echo "Checking latest version of Rust Analyzer on GitHub"
RUST_ANALYZER_RELEASES=$TMPDIR/rust-analyzer-tags
curl -sS 'https://api.github.com/repos/rust-lang/rust-analyzer/releases' > $RUST_ANALYZER_RELEASES
version=$(cat $RUST_ANALYZER_RELEASES | jq -r 'map(select(.prerelease == false)) | .[0].name')
download="https://github.com/rust-lang/rust-analyzer/releases/download/$version/rust-analyzer-linux-x64.vsix"
file=$(basename "$download")
if [ ! -f "$file" ]; then
    echo "Downloading $file (version $version)"
    curl -Os "$download"
    echo "Installing $file (version $version)"
    codium --install-extension "$file"
fi

for id in \
    dbaeumer.vscode-eslint \
    esbenp.prettier-vscode \
    golang.go \
    lextudio.restructuredtext \
    llvm-vs-code-extensions.vscode-clangd \
    ms-python.python \
    stkb.rewrap \
    streetsidesoftware.code-spell-checker \
    tamasfe.even-better-toml \
    usernamehw.errorlens \
    vscodevim.vim \
; do
    echo "Checking latest version of $id"
    download=$(curl -s "https://open-vsx.org/api/-/query?extensionId=$id" | jq -r '.extensions[0].files.download')
    if [ -z "$download" ] || [ "$download" = "null" ]; then
        echo "Extension $id not found"
        exit 1
    fi
    file=$(basename "$download")
    if [ ! -f "$file" ]; then
        echo "Downloading $file"
        rm -f "$id-*.vsix"
        curl -Os "$download"
        echo "Installing $file"
        codium --install-extension "$file"
    fi
done

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
