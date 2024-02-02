#!/bin/sh
set -eu
cd

echo $PATH

tldr --update

tar --create --file provides.tar .cache/tlrc
