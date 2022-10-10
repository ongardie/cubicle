#!/bin/sh

set -eux

opam switch list
opam env --check
