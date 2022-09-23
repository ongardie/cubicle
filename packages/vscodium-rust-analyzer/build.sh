#!/bin/sh
set -eu

# Get the rust-analyzer extension from GitHub releases, since the version on
# OpenVSX is stale and somewhat broken. See
# <https://github.com/rust-lang/rust-analyzer/issues/11080>.
echo "Checking latest version of Rust Analyzer on GitHub"
RUST_ANALYZER_RELEASES=rust-analyzer-tags
curl -sS 'https://api.github.com/repos/rust-lang/rust-analyzer/releases' > $RUST_ANALYZER_RELEASES
version=$(cat $RUST_ANALYZER_RELEASES | jq -r 'map(select(.prerelease == false)) | .[0].name')
file="rust-analyzer-linux-x64-$version.vsix"
download="https://github.com/rust-lang/rust-analyzer/releases/download/$version/rust-analyzer-linux-x64.vsix"
if [ ! -f "$file" ]; then
    echo "Downloading $file"
    curl -L "$download" -o "$file"
fi

echo "Installing $file"
codium --install-extension "$file"

cd
tar --create --file provides.tar .vscode-oss/extensions/rust-lang.rust-analyzer-*
