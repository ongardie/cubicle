#!/bin/sh
set -e

# Note: dependencies for a full Python build are listed here and are expected to
# be installed on the system already:
# https://devguide.python.org/setup/#linux

have=$(python3 --version)
echo "Have ${have:-no python3 version}"
echo "Checking latest version of Python on GitHub"
TAGS=$TMPDIR/python-tags
curl -sS 'https://api.github.com/repos/python/cpython/tags' > $TAGS
version=$(cat $TAGS | jq -r '.[] | .name' | grep -E '^v[0-9]+.[0-9]+.[0-9]+$' | sort --version-sort | tail -n 1 | cut -b2-)
if [ "$have" = "Python $version" ] && [ "$(which python3)" -nt /usr/lib/x86_64-linux-gnu/libssl.a ]; then
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
    ln -fs $version ~/opt/python/latest
    ln -fs python3 ~/opt/python/latest/bin/python
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
    'pylama[all]' \
    | pv -i 0.1 -l -N packages >/dev/null

cat > ~/.pylama.ini <<EOF
[pylama]
linters=eradicate,isort,mccabe,mypy,pycodestyle,pydocstyle,pyflakes,pylint,radon,vulture
EOF

tar -c -C ~ --verbatim-files-from --files-from ~/$SANDBOX/provides.txt -f ~/provides.tar
