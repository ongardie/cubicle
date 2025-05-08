#!/bin/sh
set -eux

echo "Checking latest version of Nushell on GitHub"
RELEASES=nushell-releases.json
curl -sS 'https://api.github.com/repos/nushell/nushell/releases' > $RELEASES

machine=$(uname -m)
download=$(cat $RELEASES | jq -r '.[0].assets | map(.browser_download_url | select(test("/nu-.*-'"$machine"'-unknown-linux-gnu.*\\.tar\\.gz$")))[0]')
tarball=$(basename "$download")

if [ -z "$download" ] || [ "$download" = "null" ]; then
    exit 1
fi

if ! [ -f "$tarball" ]; then
    curl -LO "$download"
fi

rm -rf ~/opt/nushell
mkdir -p ~/opt/nushell
tar -C ~/opt/nushell --extract --strip-components=1 --file "$tarball"

ln -fs ../opt/nushell/nu ~/bin/

mkdir -p ~/.local/share/nushell-plugins/
for p in formats gstat inc polars query; do
    ln -fs "../../../opt/nushell/nu_plugin_$p" ~/.local/share/nushell-plugins/
done

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
