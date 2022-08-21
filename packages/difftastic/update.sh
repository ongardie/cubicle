#!/bin/sh
set -eu
cargo install difftastic
tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
