#!/usr/bin/env nu

print "Checking latest version of asdf on GitHub"
let releases = "~/w/asdf-releases.json" | path expand
http get 'https://api.github.com/repos/asdf-vm/asdf/releases' | save -f $releases

let arch = match (uname | get machine) {
    "aarch64" => "arm64",
    "x86_64" => "amd64",
    _ => {
        print "Architecture not supported:"
        uname
        exit 1
    }
}

let release = (
    open $releases
    | where not prerelease
    | first
    | get assets
    | where name =~ $"^asdf-v.*-linux-($arch).tar.gz$"
    | first
    | select id name size updated_at browser_download_url
)
print $release

cd $env.TMPDIR

if ($release.name | path exists) == false {
    print $"Downloading ($release.name)"
    http get $release.browser_download_url | save $release.name
}

tar -C ~/bin -xvf $release.name

'$HOME/.asdf/shims' | save -f ~/.config/profile.d/path/50-asdf

mkdir ~/.local/share/bash-completion/completions/
asdf completion bash | save -f ~/.local/share/bash-completion/completions/asdf

mkdir ~/.config/nushell/autoload/
asdf completion nushell | save -f ~/.config/nushell/autoload/80-asdf.nu

mkdir ~/.zfunc
asdf completion zsh | save -f ~/.zfunc/_asdf

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
