#!/bin/sh
set -eu

go version

cat > hello.go << EOF
package main

import "fmt"

func main() {
	fmt.Println("Hello, World!")
}
EOF

[ "$( go run hello.go )" = "Hello, World!" ]

dlv version >/dev/null
gopls version >/dev/null
staticcheck hello.go
