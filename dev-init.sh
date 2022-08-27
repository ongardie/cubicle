#!/bin/sh
set -eu

cd

# Note: Used for bwrap only, not for Docker.
if [ -f /dev/shm/seed.tar ]; then
    echo "Unpacking seed tarball..."
    pv -i 0.1 /dev/shm/seed.tar | tar --ignore-zero --extract
    rm /dev/shm/seed.tar
fi

mkdir -p .dev-init bin opt tmp w

if [ -f ./.profile ]; then
    set +u
    . ./.profile
    set -u
fi

for f in ./.dev-init/*; do
    if [ -x "$f" ]; then
        "$f"
    fi
done

cd w
# We want this to fail if `update.sh` isn't executable, so check with `-e`
# instead of `-x`.
if [ -e ./update.sh ]; then
    echo "Running ~/w/update.sh"
    ./update.sh
fi
