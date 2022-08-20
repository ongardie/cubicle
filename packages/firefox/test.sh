#!/bin/sh
set -eu

version="$(firefox --version)"

firefox --screenshot

[ "$( xdg-mime query default x-scheme-handler/https )" = "firefox.desktop" ]
[ "$( xdg-mime query default x-scheme-handler/http )" = "firefox.desktop" ]

[ "$( sensible-browser --version )" = "$version" ]

# `timeout`` was somehow causing this to hang without `--foreground` under sudo
# to a second user account within a Docker container.
timeout --foreground --verbose 10s \
    firefox --screenshot https://example.org/
[ -f screenshot.png ]
