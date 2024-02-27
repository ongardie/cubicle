# Cubicle Packages

Cubicle uses packages to inject program files and configuration into new
environments. Use the `cub package list` subcommand to see available packages.

Packages are built automatically when they are first used, and they are updated
when they are used if:

- 12 hours have elapsed (to change this, set `auto_update` to `never`, `1h`,
  `3.5 days`, etc in the configuration file),
- their package definitions have changed, or
- one of their dependencies or build-dependencies has been updated more
  recently.

## Special packages

- The [default](../packages/default/README.md) package is used for new
  environments when a package list is not otherwise specified.

- The [`auto`](../packages/auto/README.md) package is an implicit dependency
  for every normal environment (excluding package builder/test environments).
  This is useful for configuration files, for example.

- The [`auto-batch`](../packages/auto-batch/README.md) package is an implicit
  dependency for every package build and test environment (except those that
  `auto-batch` itself transitively depends on). Compared to `auto`, this is
  used for a smaller number of critical configuration files that change less
  frequently. (If the package builders depended on `auto`, then every change to
  your `.vimrc` would result in needlessly rebuilding all packages.)

## Package Namespaces

Cubicle can also manage two types of third-party packages:

1. Depending on the runner used, Cubicle can install or verify the installation
   of operating system-level packages. For example, you can depend on
   `debian.sl` to have Cubicle install the
   [Steam Locomotive package](https://packages.debian.org/bookworm/sl) using
   `apt`. Currently, only the `debian` namespace is supported.

2. Some special Cubicle packages are designated as package managers. They can
   install user-level packages as defined by a third party. For example, the
   `crates-io` Cubicle package is a package manager that uses `cargo install`
   to install packages from <https://crates.io/>, the Rust community's package
   registry. You can depend on `crates-io.difftastic` to install the
   [Difftastic](https://crates.io/crates/difftastic) tool, even though Cubicle
   knows nothing about Difftastic.

## Package Source Locations

Cubicle looks for package definitions in the following locations:

1. Local packages in `${XDG_DATA_HOME:-~/.local/share}/cubicle/packages/*/`.
2. Built-in packages in the Cubicle source code's `packages/` directory. If
   Cubicle doesn't find this automatically, you can set `builtin_package_dir`
   in the config.

If a package with the same name appears in multiple locations, the first one is
used and the others are ignored. The sort order of the names of the containing
directories is significant for local packages, so you may want to create
`00local` to come first.

## Package Sources

A package is named after the directory containing its sources.

The directory may include other files, but Cubicle pays attention to these:

- `package.toml`: A required [TOML](https://toml.io/)-formatted file definining
  the package manifest. This is described below.

- `build.sh`: An optional executable that is run in a package builder
  environment to produce the package files. The script may download software,
  unpack it, compile it, set up configuration files, etc. It should create an
  archive at `~/provides.tar` that Cubicle will later unpack in the target
  environments' home directories. Note that `build.sh` runs when the package
  builder environment is first created and also when the package builder
  environment is updated. The package builder environments are kept around as a
  form of caching.

- `test.sh`: An optional executable that is run in a clean environment to
  sanity check the package output files. The test environment is seeded with
  the package's dependencies, the package output files, and the package source
  directory.

These files and any other files in the package directory are injected into the
work directory of the package builder environment.

The `~/provides.tar` archive is simply unpacked into the downstream
environments. Although it's ideally avoided, sometimes a package will need to
execute code to complete the setup process. If the archive contains any
executable files within `~/.dev-init/`, these will be run upon creating and
resetting target environments.

## Package Manifest

The package manifest is defined in a [TOML](https://toml.io/)-formatted file
named `package.toml`. An empty file is a valid manifest, but most packages have
something to specify. The following keys are allowed:

### `build_depends`

- Type: `map<string, {} | map<string, {}>>`
- Default: empty

This object specifies a set of dependencies that are needed only to build the
package. The package builder environment will be seeded with the listed
packages, but other environments that depend on this package will not.

The format is the same as `depends`.

### `depends`

- Type: `map<string, {} | map<string, {}>>`
- Default: empty

This object specifies a set of dependencies. Both the package builder
environment and the new environments that depend on this package will be seeded
with the listed packages.

Cubicle-level dependencies are given by the Cubicle package name as the key and
values of `{}`. Third-party dependencies are given by the Cubicle namespace as
the key and values that map from package names in the third-party namespace to
`{}`.

For example, the `rust-script` package needs both Rust and the `rust-script`
binary from `crates-io`. It lists dependencies like this:

```toml
[depends]
rust = {}

[depends.crates-io]
rust-script = {}
```

### `package_manager`

- Type: boolean
- Default: `false`

If false, this is not a package manager.

If true, this package is a special package manager that can install user-level
packages from a third party. It will not be built directly but will be built 0
or more times for different packages.

The `build.sh` script for a package manager is invoked with an environment
variable `$PACKAGE` containing the name of the third-party package to build.
