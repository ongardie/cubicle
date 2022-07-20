#!/bin/sh
set -ex

# Debian's bullseye-backports Go version seems recent enough.
go version

# Tools used by the Go VS Code extension.
go install github.com/go-delve/delve/cmd/dlv@latest
go install golang.org/x/tools/gopls@latest
go install honnef.co/go/tools/cmd/staticcheck@latest

touch ~/.UPDATED
