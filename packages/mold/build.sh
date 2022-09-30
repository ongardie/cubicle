#!/bin/sh
set -ex

mkdir -p ~/opt
cd ~/opt

if [ -d mold ]; then
    cd mold
    git fetch --all
else
    git clone https://github.com/rui314/mold.git
    cd mold
fi

TAG=$(git tag | grep -E 'v1.[0-9]+.[0-9]+' | sort --version-sort | tail -n 1)
git checkout $TAG

mkdir -p build
cd build

cmake -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_COMPILER=c++ ..
cmake --build . -j $(nproc)

cp mold ~/bin/mold
mold --version

mkdir -p ~/.dev-init
cp -a ~/w/mold-init.sh ~/.dev-init/

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
