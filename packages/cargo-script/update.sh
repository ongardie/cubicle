#!/bin/sh
set -eu
cargo install cargo-script
tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
