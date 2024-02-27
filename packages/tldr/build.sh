#!/bin/sh
set -eu
cd

tldr --update

tar --create --file provides.tar .cache/tlrc
