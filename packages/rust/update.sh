#!/bin/sh
set -ex

export PATH=~/.cargo/bin:$PATH

if ! rustup run stable echo rustup ok; then
    curl -sSfO 'https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init'
    chmod +x rustup-init
    # Exclude rust-docs component since it's too many files and too large.
    ./rustup-init -y --profile=minimal --component=rustfmt,clippy
    rm rustup-init
fi

rustup toolchain install nightly
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

touch ~/.UPDATED
