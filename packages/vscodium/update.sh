#!/bin/sh
set -e

curl() {
    command curl --connect-timeout 10 --location --max-time 20 --show-error "$@"
}

echo "Checking latest version of VS Codium"
RELEASES=$TMPDIR/vscodium-releases
curl -s 'https://api.github.com/repos/VSCodium/vscodium/releases' > $RELEASES
latest=$(jq -r '.[] | .tag_name' < $RELEASES | sort --version-sort | tail -n 1)
if [ v"$( ~/opt/vscodium/bin/codium --version | head -n 1 )" = v"$latest" ]; then
    echo "Have latest VS Codium already ($latest)"
else
    rm -rf ~/opt/vscodium
    mkdir -p ~/opt/vscodium
    cd ~/opt/vscodium
    echo "Downloading VSCodium $latest"
    curl --max-time 120 "https://github.com/VSCodium/vscodium/releases/download/$latest/VSCodium-linux-x64-$latest.tar.gz" | tar -xz
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
    download=$(curl -s "https://open-vsx.org/api/-/query?extensionId=$id" | jq -r '.extensions[0].files.download')
    if [ -z "$download" ] || [ "$download" = "null" ]; then
        echo "Extension $id not found"
        exit 1
    fi
    file=$(basename "$download")
    if [ ! -f $file ]; then
        echo "Downloading $file"
        rm -f "$id-*.vsix"
        curl -Os "$download"
    fi
    if [ ! -d ~/.vscode-oss/extensions/$(basename "$file" .vsix | tr '[:upper:]' '[:lower:]') ]; then
        echo "Installing $file"
        codium --install-extension "$file"
    fi
done

cat > ~/.dev-init/vscodium.sh <<"EOF"
#!/bin/sh
if [ ! -f ~/.config/VSCodium/User/settings.json ] && [ -f ~/configs/vscodium-settings.json ]; then
    mkdir -p ~/.config/VSCodium/User
    (
        head -n -1 ~/configs/vscodium-settings.json
        echo '  "security.workspace.trust.enabled": false,'
        echo '}'
    ) > ~/.config/VSCodium/User/settings.json
fi
EOF
chmod +x ~/.dev-init/vscodium.sh

tar -c -C ~ --verbatim-files-from --files-from ~/$SANDBOX/provides.txt -f ~/provides.tar
