#!/usr/bin/env nu

print "Checking latest version of VS Codium"
let releases = http get 'https://api.github.com/repos/VSCodium/vscodium/releases'
let latest = $releases | get tag_name.0
let have = try {
    ~/opt/vscodium/bin/codium --version | lines | first
} catch {
    "none"
}
if $have == $latest {
    print $"Have latest VS Codium already \(($latest)\)"
} else {
    let download = match (uname | get machine) {
        "aarch64" => $"VSCodium-linux-arm64-($latest)"
        "x86_64" => $"VSCodium-linux-x64-($latest)"
        _ => {
            print $"Architecture not supported:"
            uname
            exit 1
        }
    }

    rm -rf ~/opt/vscodium
    mkdir ~/opt/vscodium
    cd ~/opt/vscodium
    print $"Downloading ($download)"
    http get $"https://github.com/VSCodium/vscodium/releases/download/($latest)/($download).tar.gz"
        | tar -xz
}

cp ~/w/codium ~/bin/codium

mkdir ~/.dev-init
cp ~/w/vscodium-extensions.sh ~/.dev-init/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
