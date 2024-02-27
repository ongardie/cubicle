#!/bin/zsh

# This looks for relevant package names when you run an unknown command.
if [ -f /etc/zsh_command_not_found ]; then
    # This is from the command-not-found package.
    . /etc/zsh_command_not_found
elif command -v apt-binary >/dev/null; then
    # This is used when the command-not-found package isn't installed but my
    # apt-binary script is available.
    command_not_found_handler() {
        echo "Command '$1' not found"
        # Re-check to avoid infinite loop if apt-binary was removed.
        if command -v apt-binary >/dev/null; then
            echo "Searching Debian packages..."
            apt-binary "$1"
        fi
        return 127
    }
fi
