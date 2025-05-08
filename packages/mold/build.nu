#!/usr/bin/env nu

print "Checking latest version of mold on GitHub"
http get 'https://api.github.com/repos/rui314/mold/releases' | save -f mold-releases.json

let release = (
    open mold-releases.json
    | where not prerelease
    | first
    | get assets
    | where name =~ $"^mold-.*-(uname | get machine)-linux.tar.gz$"
    | first
    | select id name size updated_at browser_download_url
)
print $release

if ($release.name | path exists) == false {
    print $"Downloading ($release.name)"
    http get $release.browser_download_url | save $release.name
}

mkdir ~/opt/mold
tar -C ~/opt/mold --extract --strip-components=1 --file $release.name

ln -fs ../opt/mold/bin/mold ~/bin/
ln -fs ../opt/mold/bin/ld.mold ~/bin/
ln -fs ../opt/mold/bin/ld64.mold ~/bin/

mkdir ~/.dev-init
cp ~/w/mold-init.sh ~/.dev-init/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
