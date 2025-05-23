#!/bin/sh

export OPAM_SWITCH_PREFIX="$HOME/.opam/default"
export CAML_LD_LIBRARY_PATH="$HOME/.opam/default/lib/stublibs:$HOME/.opam/default/lib/ocaml/stublibs:$HOME/.opam/default/lib/ocaml"
export OCAML_TOPLEVEL_PATH="$HOME/.opam/default/lib/toplevel"
export OCAMLTOP_INCLUDE_PATH="$HOME/.opam/default/lib/toplevel"
export PKG_CONFIG_PATH="$HOME/.opam/default/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export MANPATH="${MANPATH:-}:$HOME/.opam/default/man"
