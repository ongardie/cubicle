#!/usr/bin/env nu

ln -fs /bin/true ~/bin/no-op

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
