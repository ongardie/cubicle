#!/bin/sh
set -eu

tldr --update

cd
tar --create --file provides.tar .tldr
