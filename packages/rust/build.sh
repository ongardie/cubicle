#!/bin/sh
set -ex

export PATH="$HOME/.cargo/bin:$PATH"

# shellcheck disable=SC2016
echo '$HOME/.cargo/bin' > ~/.config/profile.d/path/33-cargo

if ! rustup run stable echo rustup ok; then
    # Exclude rust-docs component since it's too many files and too large.
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
        -y --profile=minimal --component=rustfmt,clippy
fi

rustup update

BASH_COMP=~/.local/share/bash-completion/completions
ZSH_COMP=~/.zfunc
mkdir -p $BASH_COMP $ZSH_COMP
rustup completions bash > $BASH_COMP/rustup
rustup completions zsh > $ZSH_COMP/_rustup
rustup completions bash cargo > $BASH_COMP/cargo
rustup completions zsh cargo > $ZSH_COMP/_cargo

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
