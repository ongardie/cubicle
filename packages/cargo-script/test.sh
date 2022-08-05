#!/bin/sh
set -eu

cat > hello.rs << EOF
#!/usr/bin/env run-cargo-script

fn main() {
    println!("Hello, World!");
}
EOF

[ "$(cargo script hello)" = "Hello, World!" ]

chmod +x hello.rs
[ "$(./hello.rs)" = "Hello, World!" ]
