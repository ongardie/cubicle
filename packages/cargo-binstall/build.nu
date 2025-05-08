#!/usr/bin/env nu

cd $env.TMPDIR
let arch = uname | get machine
let name = $"cargo-binstall-($arch)-unknown-linux-musl.tgz"

http get $"https://github.com/cargo-bins/cargo-binstall/releases/latest/download/($name)" | save -f $name
tar -xf $name
mkdir ~/.cargo/bin
mv cargo-binstall ~/.cargo/bin

cd
mkdir .config/profile.d
cp w/50-cargo-binstall.sh .config/profile.d/50-cargo-binstall.sh

tar --create --file provides.tar .cargo/bin/cargo-binstall .config/profile.d/50-cargo-binstall.sh
