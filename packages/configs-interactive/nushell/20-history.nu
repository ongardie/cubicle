#!/usr/bin/env nu

$env.config.history.file_format = 'sqlite'

# There doesn't seem to be a way to set a custom history path.
# See https://github.com/nushell/nushell/issues/11962.
# The `nushell-history.sh` script sets up symlinks as a workaround.
