#!/usr/bin/env nu

mkdir ~/.config/bashrc.d/
cp bashrc.d/* ~/.config/bashrc.d/

mkdir ~/.config/nushell/autoload/
cp nushell/* ~/.config/nushell/autoload/

mkdir ~/.config/shrc.d/
cp shrc.d/* ~/.config/shrc.d/

mkdir ~/.config/zshrc.d/
cp zshrc.d/* ~/.config/zshrc.d/

mkdir ~/.dev-init/
cp nushell-history.sh ~/.dev-init/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
