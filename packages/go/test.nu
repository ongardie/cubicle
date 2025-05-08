#!/usr/bin/env nu

use std/assert

go version

'package main

import "fmt"

func main() {
	fmt.Println("Hello, World!")
}
' | save -f hello.go

assert equal (go run hello.go) "Hello, World!"

go install golang.org/x/example/hello@latest
assert equal (hello -r) "olleH, dlrow!"
