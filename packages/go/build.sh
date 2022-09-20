#!/bin/sh
set -eu
cd

CACHE_HOME=${XDG_CACHE_HOME:-"$HOME/.cache"}
mkdir -p "$CACHE_HOME"

echo "Checking latest version of Go"
RELEASES="$CACHE_HOME/go-releases.json"
curl -s 'https://go.dev/dl/?mode=json' > "$RELEASES"
version=$(jq -r 'map(select(.stable) | .version)[0]' < "$RELEASES")

installed=$(go env GOVERSION || true)
installed_root=$(go env GOROOT || true)
echo "Have $installed installed at $installed_root"

if [ "$installed" != "$version" ] || [ "$installed_root" != "$HOME/opt/go" ]; then
    if [ ! -f "$version.linux-amd64.tar.gz" ]; then
        echo "Downloading $version"
        curl -LO "https://go.dev/dl/$version.linux-amd64.tar.gz"
    fi

    echo "Unpacking $version"
    mkdir -p ~/opt
    rm -rf ~/opt/go
    pv "$version.linux-amd64.tar.gz" | tar -xz -C ~/opt
fi

mkdir -p ~/bin
ln -fs ../opt/go/bin/go ~/bin/

installed=$(go env GOVERSION || true)
installed_root=$(go env GOROOT || true)
if [ "$installed" != "$version" ] || [ "$installed_root" != "$HOME/opt/go" ]; then
    echo "ERROR: Have $installed installed at $installed_root, which is still not right"
    exit 1
fi

set -x

# Tools used by the Go VS Code extension.
go install github.com/go-delve/delve/cmd/dlv@latest
go install golang.org/x/tools/gopls@latest
go install honnef.co/go/tools/cmd/staticcheck@latest

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar