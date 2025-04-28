#!/usr/bin/env nu

alias apt-binary = apt-binary.nu

# This looks for relevant package names when you run an unknown command. It
# uses the command-not-found package if installed or my apt-binary.nu script,
# otherwise.
$env.config.hooks.command_not_found = { |name|
    if (which command-not-found | is-not-empty) {
        command-not-found $name
    } else if (which ^apt-binary.nu | is-not-empty) {
        print 'Searching Debian packages...'
        apt-binary $name
    }

    return null
}
