## auto-batch package

The `auto-batch` package is an implicit dependency for every package build and
test environment (except those that `auto-batch` itself transitively depends
on).

See [`auto`](../auto/README.md) for a similar package that is an implicit
dependency for every normal, interactive environment.

Compared to `auto`, `auto-batch` is used for a smaller number of critical
configuration files that change less frequently. (If the package builders
depended on `auto`, then every change to your `.vimrc` would result in
needlessly rebuilding all packages.)
