#!/bin/bash

# Only for interactive shells.
if [[ "$-" = *i* ]]; then
    # Disable tty flow control (Ctrl-S and Ctrl-Q).
    stty -ixon
    bind '"\C-s": self-insert'
    bind '"\C-q": self-insert'
fi
