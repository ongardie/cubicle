#!/usr/bin/env nu

$env.config.history = {
    max_size: 100_000,
    sync_on_enter: true,
    file_format: 'sqlite',
    isolation: true,
}

# There doesn't seem to be a way to set a custom history path.
# See https://github.com/nushell/nushell/issues/11962.
# The `nushell-history.sh` script sets up symlinks as a workaround.
