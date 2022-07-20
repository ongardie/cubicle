#!/bin/sh
set -e

if [ -f /dev/shm/seed.tar ]; then
    echo "Unpacking seed tarball..."
    pv -i 0.1 /dev/shm/seed.tar | tar -C ~ -x
    rm /dev/shm/seed.tar
fi

if [ -f ~/.profile ]; then
    . ~/.profile
fi

if [ ! -f ~/.config/VSCodium/User/settings.json ] && [ -f ~/configs/vscodium-settings.json ]; then
    mkdir -p ~/.config/VSCodium/User
    (
        head -n -1 ~/configs/vscodium-settings.json
        echo '  "security.workspace.trust.enabled": false,'
        echo '}'
    ) > ~/.config/VSCodium/User/settings.json
fi

# This writes to `~/.config/mimeapps.list`.
xdg-mime default firefox-esr.desktop x-scheme-handler/https x-scheme-handler/http

if [ -x ~/$SANDBOX/update.sh ]; then
    echo "Running ~/$SANDBOX/update.sh"
    ~/$SANDBOX/update.sh
fi
