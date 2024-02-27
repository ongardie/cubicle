#!/bin/bash

if [ -n "${CUBICLE:-}" ]; then
    HISTFILE="$HOME/w/.bash_history"
fi

HISTCONTROL=ignoreboth
HISTFILESIZE=100000
HISTSIZE=100000
shopt -s histappend
