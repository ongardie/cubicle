#!/bin/sh

set -eu

H="$HOME/w/.nushell"
C="$HOME/.config/nushell"

if ! [ -f "$H/history.sqlite3" ]; then
    mkdir -p "$H"
    touch \
        "$H/history.sqlite3" \
        "$H/history.sqlite3-shm" \
        "$H/history.sqlite3-wal"
fi

if ! [ -f "$C/history.sqlite3" ]; then
    mkdir -p "$C"
    ln -fs \
        "$H/history.sqlite3" \
        "$H/history.sqlite3-shm" \
        "$H/history.sqlite3-wal" \
        "$C/"
fi
