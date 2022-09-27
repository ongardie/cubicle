#!/bin/sh
set -e

# Note: this Python installation ends up installed in `$HOME/opt/python/` and
# cannot be relocated from there, not even to another user's
# `$HOME/opt/python/`. Therefore, this package is currently unusable under the
# `users` runner.

# Note: dependencies for a full Python build are listed here and are expected to
# be installed on the system already:
# https://devguide.python.org/setup/#linux

export PATH="$HOME/opt/python/latest/bin:$PATH"
echo "$HOME/opt/python/latest/bin" > ~/.configs/profile.d/path/37-python

have=$(python3 --version || true)
echo "Have ${have:-no python3 version}"
echo "Checking latest version of Python on GitHub"
TAGS=$TMPDIR/python-tags
curl -sS 'https://api.github.com/repos/python/cpython/tags' > $TAGS
version=$(cat $TAGS | jq -r '.[] | .name' | grep -E '^v[0-9]+.[0-9]+.[0-9]+$' | sort --version-sort | tail -n 1 | cut -b2-)
if [ "$have" = "Python $version" ] && [ "$(which python3)" -nt "/usr/lib/$(uname -m)-linux-gnu/libssl.a" ]; then
    echo "Have latest Python already ($version)"
else
    echo "Downloading and installing Python $version"
    cd
    if [ ! -f Python-$version.tar.xz ]; then
        curl -LOsS "https://www.python.org/ftp/python/$version/Python-$version.tar.xz"
    fi
    if [ ! -d Python-$version ]; then
        tar -xf Python-$version.tar.xz
    fi
    cd Python-$version
    mkdir -p ~/opt/python
    ./configure --prefix ~/opt/python/$version
    make -j
    make install
    ln -fns $version ~/opt/python/latest
    ln -fs python3 ~/opt/python/latest/bin/python
    ln -fs pip3 ~/opt/python/latest/bin/pip
fi

mkdir -p ~/opt/python/latest/sbin
for f in \
    ipython \
    ipython3 \
    pip \
    pip3 \
    pydoc3 \
    python \
    python3 \
    python3-config \
; do
    ln -fs ../bin/$f ~/opt/python/latest/sbin/
done

if [ "$(python3 --version)" != "Python $version" ]; then
  echo "ERROR: Have $(python3 --version), which is still not right"
  exit 1
fi

echo 'Upgrading pip and packages'
pip3 install --upgrade pip

pip3 install --upgrade \
    black \
    ipython \
    pyflakes \
    'pylama[all]' \
    shiv \
    | pv -i 0.1 -l -N packages >/dev/null

cat > ~/.pylama.ini <<EOF
[pylama]
linters=eradicate,isort,mccabe,mypy,pycodestyle,pydocstyle,pyflakes,pylint,radon,vulture
EOF

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
