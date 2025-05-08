#!/usr/bin/env nu

cd

go install $"($env.PACKAGE)@latest"
asdf reshim golang

let bin = $env.PACKAGE | path basename
let install = [
    (asdf where golang | path relative-to ~)
    bin
    $bin
] | path join
let shim = ".asdf/shims/" | path join $bin

tar --create --verbose --file provides.tar $install $shim
