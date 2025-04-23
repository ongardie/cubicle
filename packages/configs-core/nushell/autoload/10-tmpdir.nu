#!/usr/bin/env nu

$env.TMPDIR = $env.HOME | path join tmp
mkdir $env.TMPDIR
