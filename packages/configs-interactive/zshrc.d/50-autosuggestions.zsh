#!/bin/zsh

# Enable autosuggestions. On Debian, this is provided by the
# zsh-autosuggestions package.

if [ -f /usr/share/zsh-autosuggestions/zsh-autosuggestions.zsh ]; then
    . /usr/share/zsh-autosuggestions/zsh-autosuggestions.zsh
    ZSH_AUTOSUGGEST_STRATEGY=(history completion)
fi
