#!/bin/sh
set -e

# Note: Used for bwrap, not for Docker.
if [ -f /dev/shm/seed.tar ]; then
    echo "Unpacking seed tarball..."
    pv -i 0.1 /dev/shm/seed.tar | tar --ignore-zero --directory ~ --extract
    rm /dev/shm/seed.tar
fi

mkdir -p ~/.dev-init ~/bin ~/opt ~/tmp


if [ -f ~/.profile ]; then
    . ~/.profile
fi

for f in ~/.dev-init/*; do
    if [ -x "$f" ]; then
      $f
    fi
done

# We want this to fail if `update.sh` isn't executable, so check with `-e`
# instead of `-x`.
if [ -e ~/$SANDBOX/update.sh ]; then
    echo "Running ~/$SANDBOX/update.sh"
    ~/$SANDBOX/update.sh
fi
