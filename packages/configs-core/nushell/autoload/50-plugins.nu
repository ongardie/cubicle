#!/usr/bin/env nu

use std/assert

# Note: Registered plugin filenames have absolute paths and resolved symlinks.
def registered [] {
    plugin list | get filename
}

for file in (
    glob ~/.local/share/nushell-plugins/nu_plugin_*
    | path expand
    | where $it not-in (registered)
) {
    plugin add $file
    assert ($file in (registered))
}
