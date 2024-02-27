#!/bin/zsh

setopt EXTENDED_HISTORY
setopt HIST_FIND_NO_DUPS
setopt HIST_IGNORE_SPACE
setopt INC_APPEND_HISTORY

if [ -n "${CUBICLE:-}" ]; then
    export HISTFILE="$HOME/w/.zsh_history"
else
    export HISTFILE="$HOME/.zsh_history"
fi

export HISTSIZE=100000
export SAVEHIST=100000
