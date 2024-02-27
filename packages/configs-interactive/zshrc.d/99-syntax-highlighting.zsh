#!/bin/zsh

# Enable syntax higlighting. On Debian, this is provided by the
# zsh-syntax-highlighting package. This needs to come after any other modules
# are registered.

if [ -f /usr/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh ]; then
    . /usr/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh
fi
