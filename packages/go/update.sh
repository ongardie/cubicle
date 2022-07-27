#!/bin/sh
set -eu

CACHE_HOME=${XDG_CACHE_HOME:-"$HOME/.cache"}
mkdir -p "$CACHE_HOME"

echo "Checking latest version of Go"
RELEASES="$CACHE_HOME/go-releases.json"
curl -s 'https://go.dev/dl/?mode=json' > "$RELEASES"
version=$(jq -r 'map(select(.stable) | .version)[0]' < "$RELEASES")

installed=$(go env GOVERSION || true)
echo "Have $installed"

if [ x"$installed" != x"$version" ]; then
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
ln -fs ~/opt/go/bin/go ~/bin/

if [ x"$(go env GOVERSION)" != x"$version" ]; then
    echo "ERROR: Have $(go env GOVERSION), which is still not right"
    exit 1
fi

set -x

# Tools used by the Go VS Code extension.
go install github.com/go-delve/delve/cmd/dlv@latest
go install golang.org/x/tools/gopls@latest
go install honnef.co/go/tools/cmd/staticcheck@latest

touch ~/.UPDATED
