## auto package

The `auto` package is an implicit dependency for every normal Cubicle
environment (excluding package builder/test environments). Users may want to
shadow this package locally to customize configuration files or depend on
globally useful tools.

See [`auto-batch`](../auto-batch/README.md) for a similar package that is an
implicit dependency for every package builder/test environment.

This is a "virtual" package that depends on
[configs-interactive](../configs-interactive/README.md). To customize their
configuration, users can shadow this package, keep its dependency on
`configs-interactive`, and add their own configuration files in their local
copy of the `auto` package.
