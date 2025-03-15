#!/bin/sh

set -eux

cargo binstall --no-confirm rust-script

cat > script.rs <<'END'
#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! rand = "0.8.0"
//! ```

use rand::prelude::*;

fn main() {
    let x: u64 = random();
    println!("A random number: {}", x);
}
END

chmod +x script.rs

./script.rs
