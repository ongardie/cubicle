#!/bin/sh
set -eu
cargo install cargo-script
tar -c -C ~ --verbatim-files-from --files-from ~/$SANDBOX/provides.txt -f ~/provides.tar
