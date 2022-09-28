#!/bin/sh
set -ex

export PATH="$HOME/.cargo/bin:$PATH"
echo "$HOME/.cargo/bin" > ~/.configs/profile.d/path/33-cargo

if ! rustup run stable echo rustup ok; then
    # Exclude rust-docs component since it's too many files and too large.
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
        -y --profile=minimal --component=rustfmt,clippy
fi

rustup update

# Update `~/.cargo/registry/index`. This previously used `cargo search`, which
# will populate the index initially but won't update it. `cargo add` seems more
# reliable.
echo "Updating Cargo registry index"
TMP=$(mktemp -d)
(
    cd $TMP
    cargo init --vcs=none --name=tmp
    cargo add log
)
rm -r $TMP

BASH_COMP=~/.local/share/bash-completion/completions
ZSH_COMP=~/.zfunc
mkdir -p $BASH_COMP $ZSH_COMP
rustup completions bash > $BASH_COMP/rustup
rustup completions zsh > $ZSH_COMP/_rustup
rustup completions bash cargo > $BASH_COMP/cargo
rustup completions zsh cargo > $ZSH_COMP/_cargo

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
