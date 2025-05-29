#!/usr/bin/env nu

$env.PATH = [($env.HOME | path join .cargo bin)] ++ $env.PATH
'$HOME/.cargo/bin' | save -f ~/.config/profile.d/path/33-cargo

try {
    rustup run stable echo rustup ok
    rustup update
} catch {
    # Exclude rust-docs component since it's too many files and too large.
    http get 'https://sh.rustup.rs'
        | sh -s -- -y --profile=minimal --component=rustfmt,clippy
}

let bash_comp = "~/.local/share/bash-completion/completions" | path expand
let zsh_comp = "~/.zfunc" | path expand
mkdir $bash_comp $zsh_comp
rustup completions bash cargo | save -f ($bash_comp | path join cargo)
rustup completions bash rustup | save -f ($bash_comp| path join rustup)
rustup completions zsh cargo | save -f ($zsh_comp | path join _cargo)
rustup completions zsh rustup | save -f ($zsh_comp | path join _rustup)

mkdir ~/.cargo/config.d/
cp ~/w/10-dev-debug.toml ~/.cargo/config.d/

mkdir ~/.dev-init
cp ~/w/50-cargo-config.nu ~/.dev-init/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
