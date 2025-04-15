#!/bin/sh

set -eu

if [ "$(jj config get user.name)" = "" ]; then
    git_name="$(git config --get user.name)"
    if [ "$git_name" != "" ]; then
        jj config set --user user.name "$git_name"
    fi
fi

if [ "$(jj config get user.email)" = "" ]; then
    git_email="$(git config --get user.email)"
    if [ "$git_email" != "" ]; then
        jj config set --user user.email "$git_email"
    fi
fi
