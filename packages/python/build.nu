#!/usr/bin/env nu

# Note: this Python installation ends up installed in `$HOME/opt/python/` and
# cannot be relocated from there, not even to another user's
# `$HOME/opt/python/`. Therefore, this package is currently unusable under the
# `users` runner.

# Note: dependencies for a full Python build are listed here and are expected to
# be installed on the system already:
# https://devguide.python.org/setup/#linux

$env.PATH = [($env.HOME | path join opt python latest bin)] ++ $env.PATH
"$HOME/opt/python/latest/bin" | save -f ~/.config/profile.d/path/37-python

let have = try {
    (python3 --version)
} catch {
    ""
}
if have == "" {
    print "python3 not installed"
} else {
    print $"Have ($have)"
}

print "Checking latest version of Python on GitHub"
let tags = http get 'https://api.github.com/repos/python/cpython/tags'
let version = (
    $tags
    | get name
    | where $it =~ '^v[0-9]+\.[0-9]+\.[0-9]+$'
    | first
    | str substring 1..
)

def local_install [] {
    try {
        which python3 | get path.0 | path relative-to ~
        true
    } catch {
        false
    }
}

def newer_than_libssl [] {
    let built = (
        which python3
        | get path.0
        | ls $in
        | get modified.0
    )
    ls /usr/lib/*/libssl.a
        | get modified
        | all { $in < $built }
}

if (local_install) and $have == $"Python ($version)" and (newer_than_libssl) {
    print $"Have latest Python already \(($version)\)"
} else {
    print $"Downloading and installing Python ($version)"
    cd
    let tarball = $"Python-($version).tar.xz"
    if ($tarball | path exists) == false {
        http get $"https://www.python.org/ftp/python/($version)/($tarball)"
            | save -f $tarball
    }
    let dir = $"Python-($version)"
    if ($dir | path type) != "dir" {
        tar -xf $tarball
    }
    cd $dir
    mkdir ~/opt/python
    ./configure --prefix ([$env.HOME opt python $version] | path join)
    make -j
    make install
    ln -fns $version ~/opt/python/latest
    ln -fs python3 ~/opt/python/latest/bin/python
    ln -fs pip3 ~/opt/python/latest/bin/pip
}

mkdir ~/opt/python/latest/sbin
for f in [
    ipython
    ipython3
    pip
    pip3
    pydoc3
    python
    python3
    python3-config
] {
    ln -fs ([.. bin $f] | path join) ~/opt/python/latest/sbin/
}

if (python3 --version) != $"Python ($version)" {
    print $"ERROR: Have (python3 --version), which is still not right"
    exit 1
}

print 'Upgrading pip and packages'
pip3 install --upgrade pip

let packages = [
    black
    ipython
    pyflakes
    'pylama[all]'
    shiv
]
pip3 install --upgrade ...$packages
    | pv -i 0.1 -l -N packages out> /dev/null

'[pylama]
linters=eradicate,isort,mccabe,mypy,pycodestyle,pydocstyle,pyflakes,pylint,radon,vulture
' | save -f ~/.pylama.ini

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
