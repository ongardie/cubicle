#!/bin/sh
set -eux

echo "Checking latest version of typst on GitHub"
RELEASES=typst-releases.json
curl -sS 'https://api.github.com/repos/typst/typst/releases' > $RELEASES

machine=$(uname -m)
download=$(cat $RELEASES | jq -r 'map(select(.prerelease == false)) | .[0].assets | map(.browser_download_url | select(test("/typst-'"$machine"'-unknown-linux-musl\\.tar\\.xz$")))[0]')
tarball=$(basename "$download")

if [ -z "$download" ] || [ "$download" = "null" ]; then
    exit 1
fi

if ! [ -f "$tarball" ]; then
    curl -LO "$download"
fi

mkdir -p ~/opt/typst
tar -C ~/opt/typst --extract --strip-components=1 --file "$tarball"

ln -fs ../opt/typst/typst ~/bin/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
