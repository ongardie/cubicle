#!/bin/sh

set -eux

cd

# Default name and email from git at container init.
mkdir -p .dev-init
cp -a w/jj-user-from-git.sh .dev-init/
chmod +x .dev-init/jj-user-from-git.sh

# Bash completion.

mkdir -p .config/bashrc.d
cat > .config/bashrc.d/80-jj.bash <<END
#!/bin/bash

$(COMPLETE=bash jj)
END
chmod +x .config/bashrc.d/80-jj.bash

# Zsh completion.

mkdir -p .config/zshrc.d
cat > .config/zshrc.d/80-jj.zsh <<END
#!/bin/zsh

$(COMPLETE=zsh jj)
END
chmod +x .config/zshrc.d/80-jj.zsh

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
