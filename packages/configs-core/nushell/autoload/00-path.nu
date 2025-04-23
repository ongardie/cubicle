#!/usr/bin/env nu

let path_dir = $env.HOME | path join .config profile.d path
$env.PATH = glob ($path_dir + '/*')
    | sort
    | each { open --raw $in }
    | lines
    | str replace --regex '#.*' ''
    | str replace '$HOME' $env.HOME
    | where { is-not-empty }
