#!/bin/sh
set -eu

cd
mkdir -p .dev-init bin opt tmp w

if [ -f ./.profile ]; then
    set +u
    # shellcheck source=/dev/null
    . ./.profile
    set -u
fi

for f in ./.dev-init/*; do
    if [ -f "$f" ] && [ -x "$f" ]; then
        "$f"
    fi
done

cd w
if [ -x ./update.sh ]; then
    echo "Running ~/w/update.sh"
    ./update.sh || echo "WARNING: ~/w/update.sh in $CUBICLE failed with status $?"
fi
