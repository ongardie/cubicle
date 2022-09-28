#!/bin/sh
set -eu

asdf global golang latest

go version

cat > hello.go << EOF
package main

import "fmt"

func main() {
	fmt.Println("Hello, World!")
}
EOF

[ "$( go run hello.go )" = "Hello, World!" ]
