#!/bin/sh
set -eu

cd
bin=$(basename "$PACKAGE")
go install "$PACKAGE"@latest
asdf reshim golang
install="$(asdf where golang)/packages/bin/$bin"
install="${install#"$HOME/"}"
tar --create --verbose --file provides.tar \
    "$install" \
    ".asdf/shims/$bin"
