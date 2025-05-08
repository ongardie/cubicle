#!/usr/bin/env nu

cp bin/* ~/bin/
cp bashrc.d/* ~/.config/bashrc.d/
cp shrc.d/* ~/.config/shrc.d/
cp zshrc.d/* ~/.config/zshrc.d/

mkdir ~/.config/nushell/autoload/
cp nushell/* ~/.config/nushell/autoload/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
