#!/bin/sh
set -eu

# Note: Used for bwrap only, not for Docker.
if [ -f /dev/shm/seed.tar ]; then
    echo "Unpacking seed tarball..."
    pv -i 0.1 /dev/shm/seed.tar | tar --ignore-zero --directory "$HOME" --extract
    rm /dev/shm/seed.tar
fi

mkdir -p "$HOME/.dev-init" "$HOME/bin" "$HOME/opt" "$HOME/tmp" "$HOME/w"


if [ -f "$HOME/.profile" ]; then
    set +u
    . "$HOME/.profile"
    set -u
fi

for f in "$HOME/.dev-init/"*; do
    if [ -x "$f" ]; then
        "$f"
    fi
done

# We want this to fail if `update.sh` isn't executable, so check with `-e`
# instead of `-x`.
if [ -e "$HOME/w/update.sh" ]; then
    echo "Running $HOME/w/update.sh"
    "$HOME/w/update.sh"
fi
