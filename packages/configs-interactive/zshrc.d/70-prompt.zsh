#!/bin/zsh

parse_git_branch() {
    ref=$(git symbolic-ref HEAD -q 2>/dev/null)
    st=$?
    if [ $st -eq 1 ]; then
        echo "~~detached~~"
    elif [ $st -eq 0 ]; then
        echo "${ref#refs/heads/}"
    fi
}

prompt_command() {
    st=$?
    title=''
    visible='%B'

    # Print warning when previous command fails.
    if [ $st -ne 0 ]; then
        visible="$visible%F{red}Command exited with status $st%f\n"
    fi

    # Cubicle environment name or username & host.
    if [ -n "${CUBICLE:-}${SSH_CLIENT:-}${SUDO_USER:-}" ]; then
        visible="$visible%F{yellow}${CUBICLE:-%n@%m}%f:"
        title="$title${CUBICLE:-%n@%m}:"
    fi

    # Working directory
    visible="$visible%F{green}%~%f"
    title="$title%~"

    # Git branch
    git=$(parse_git_branch)
    if [ -n "$git" ]; then
        visible="$visible:%F{yellow}$git%f"
        title="$title:$git"
    fi

    # End prompt in '$' or '@' based on Vi mode
    if [ "${KEYMAP:-}" = 'vicmd' ]; then
        visible="$visible@"
    else
        visible="$visible$"
    fi

    # End
    visible="$visible%b "
    print -n "%{\e]2;$title\a%}$visible"
}

setopt PROMPT_SUBST
PROMPT='$(prompt_command)'

# This causes prompt_command() to be invoked whenever the keymap changes.
zle-keymap-select () {
    zle reset-prompt
}
zle -N zle-keymap-select
