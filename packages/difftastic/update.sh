#!/bin/sh
set -eu
cargo install difftastic
tar -c -C ~ --verbatim-files-from --files-from ~/$SANDBOX/provides.txt -f ~/provides.tar
