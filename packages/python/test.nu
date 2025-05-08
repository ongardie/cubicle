#!/usr/bin/env nu

use std/assert

assert equal (python -c 'print("Hello")') "Hello"

assert equal (python --version) (python3 --version)
assert equal (ipython --version) (ipython3 --version)
assert equal (pip --version) (pip3 --version)

assert ('absolute value' in (pydoc3 abs))

assert (
    python3-config --prefix
    | str starts-with ($env.HOME | path join opt python)
)

black --version out> /dev/null
pylama --version out> /dev/null
