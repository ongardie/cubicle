#!/bin/sh
set -e

if [ -f /dev/shm/seed.tar ]; then
    echo "Unpacking seed tarball..."
    pv -i 0.1 /dev/shm/seed.tar | tar --ignore-zero --directory ~ -x
    rm /dev/shm/seed.tar
fi

if [ -f ~/.profile ]; then
    . ~/.profile
fi

for f in ~/.dev-init/*; do
    if [ -x "$f" ]; then
      $f
    fi
done

if [ -x ~/$SANDBOX/update.sh ]; then
    echo "Running ~/$SANDBOX/update.sh"
    ~/$SANDBOX/update.sh
fi
