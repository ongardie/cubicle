#!/bin/sh
set -eux

echo "Checking latest version of mold on GitHub"
RELEASES=mold-releases.json
curl -sS 'https://api.github.com/repos/rui314/mold/releases' > $RELEASES

machine=$(uname -m)
download=$(cat $RELEASES | jq -r 'map(select(.prerelease == false)) | .[0].assets | map(.browser_download_url | select(test("/mold-.*-'"$machine"'-linux\\.tar\\.gz$")))[0]')
tarball=$(basename "$download")

if [ -z "$download" ] || [ "$download" = "null" ]; then
    exit 1
fi

if ! [ -f "$tarball" ]; then
    curl -LO "$download"
fi

mkdir -p ~/opt/mold
tar -C ~/opt/mold --extract --strip-components=1 --file "$tarball"

ln -fs ../opt/mold/bin/mold ~/bin/
ln -fs ../opt/mold/bin/ld.mold ~/bin/
ln -fs ../opt/mold/bin/ld64.mold ~/bin/

mkdir -p ~/.dev-init
cp -a ~/w/mold-init.sh ~/.dev-init/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
