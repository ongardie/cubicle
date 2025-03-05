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

go install golang.org/x/example/hello@latest
[ "$(hello -r)" = "olleH, dlrow!" ]
