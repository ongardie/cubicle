#!/bin/sh
set -eu

cargo init test-rust
cd test-rust
cargo run
