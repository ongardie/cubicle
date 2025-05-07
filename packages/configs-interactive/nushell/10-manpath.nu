#!/usr/bin/env nu

$env.MANPATH = [
    ($env.HOME | path join .local share man)
    /usr/local/man
    /usr/local/share/man
    /usr/share/man
] | str join ':'
