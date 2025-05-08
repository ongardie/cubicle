#!/usr/bin/env nu

cd
let dir = "opt/npm/" | path join $env.PACKAGE
mkdir $dir
cd $dir
npm install --install-strategy shallow $env.PACKAGE
cd node_modules/.bin
let links = (
    glob *
    | path relative-to (pwd)
    | each { |f|
        print $"Found executable: ($f)"
        let link_target = [
            ".."
            (pwd | path relative-to ~)
            $f
        ] | path join
        ln -fs $link_target ~/bin/
        [bin $f] | path join
    }
)

cd
tar --create --file provides.tar $dir ...$links
