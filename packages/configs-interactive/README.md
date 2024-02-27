# configs-interactive package

This package contains environment and shell configurations for interactive use.
It is used for normal Cubicle environments (via the [auto](../auto/README.md)
package). It's not used for package builder/test environments, since that would
result in needless package rebuilds when these config files change.

This package includes configurations for Bash and Zsh that should provide
pleasant and reasonable defaults for most users. It builds on the structure
defined in the [configs-core](../configs-core/README.md) package. To customize
their configurations, users should be able to shadow the "virtual"
[auto](../auto/README.md) package, keep its dependency on
`configs-interactive`, and add their own configuration files in their local
`auto` package.
