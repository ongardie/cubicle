#!/bin/bash

# This looks for relevant package names when you run an unknown command. It's
# used only when the command-not-found package isn't installed but my
# apt-binary script is available.
if ! declare -F command_not_found_handle > /dev/null &&
    command -v apt-binary >/dev/null; then
    command_not_found_handle() {
        echo "Command '$1' not found"
        # Re-check to avoid infinite loop if apt-binary was removed.
        if command -v apt-binary >/dev/null; then
            echo "Searching Debian packages..."
            apt-binary "$1"
        fi
        return 127
    }
fi
