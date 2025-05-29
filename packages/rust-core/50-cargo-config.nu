#!/usr/bin/env nu

let that = "This"
let header = $"# ($that) file was automatically generated from `~/.cargo/config.d/`."

glob --no-dir ~/.cargo/config.d/*
    | sort
    | each { open --raw | from toml }
    | reduce --fold {} {|it, acc|
        $it | merge deep --strategy append $acc
      }
    | to toml
    | $"($header)\n\n($in)"
    | save -f ~/.cargo/config.toml
