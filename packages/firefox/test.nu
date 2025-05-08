#!/usr/bin/env nu

use std/assert

let version = firefox --version

firefox --screenshot

assert equal (xdg-mime query default x-scheme-handler/https) firefox.desktop
assert equal (xdg-mime query default x-scheme-handler/http) firefox.desktop

assert equal (sensible-browser --version) $version

# `timeout`` was somehow causing this to hang without `--foreground` under sudo
# to a second user account within a Docker container.
(timeout --foreground --verbose 10s
    firefox --screenshot https://example.org/)
assert ("screenshot.png" | path exists)
