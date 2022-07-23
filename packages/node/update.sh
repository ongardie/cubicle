#!/bin/sh
set -e

# Note: this depends on ongardie's magical `node` that installs node versions
# for you. See <https://github.com/ongardie/configs/tree/main/bin>.

(
    mkdir -p ~/tmp/lts
    cd ~/tmp/lts
    echo 'lts/*' > .nvmrc
    echo "Installing/updating LTS Node.js release"
    npm update --location=global
)

(
    mkdir -p ~/tmp/current
    cd ~/tmp/current
    echo 'node' > .nvmrc
    echo "Installing/updating current Node.js release"
    npm update --location=global
)

touch ~/.UPDATED
