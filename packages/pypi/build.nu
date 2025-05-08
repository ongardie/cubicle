#!/usr/bin/env nu

# Note: this assumption is unlikely to hold for many packages.
let bin = $env.PACKAGE

cd
shiv --console-script $bin --output-file $"bin/($bin)" $env.PACKAGE
tar --create --file provides.tar $"bin/($bin)"
