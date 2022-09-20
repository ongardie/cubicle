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

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
