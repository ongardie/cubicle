#!/usr/bin/env nu

cargo binstall --no-confirm rust-script

'#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! rand = "0.8.0"
//! ```

use rand::prelude::*;

fn main() {
    let x: u64 = random();
    println!("A random number: {}", x);
}
' | save -f script.rs

chmod +x script.rs

./script.rs
