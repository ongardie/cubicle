#!/usr/bin/env nu

$env.OPAM_SWITCH_PREFIX = $env.HOME | path join .opam/default

$env.CAML_LD_LIBRARY_PATH = [
    ($env.HOME | path join .opam/default/lib/stublibs)
    ($env.HOME | path join .opam/default/lib/ocaml/stublibs)
    ($env.HOME | path join .opam/default/lib/ocaml)
] | str join ':'

$env.OCAML_TOPLEVEL_PATH = $env.HOME | path join .opam/default/lib/toplevel
$env.OCAMLTOP_INCLUDE_PATH = $env.HOME | path join .opam/default/lib/toplevel

$env.PKG_CONFIG_PATH = (
    (try { $env.PKG_CONFIG_PATH | split row ':'} catch { [] })
    | prepend ($env.HOME | path join .opam/default/lib/pkgconfig)
    | uniq
    | str join ':'
)

$env.MANPATH = (
    (try { $env.MANPATH | split row ':'} catch { [] })
    | append ($env.HOME | path join .opam/default/man)
    | uniq
    | str join ':'
)
