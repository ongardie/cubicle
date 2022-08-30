# Cubicle development container manager

Cubicle is a program to manage containers or sandbox environments. It is
intended for isolating development environments from the host system and from
each other.

Cubicle can run on top of any of these isolation mechanisms, called _runners_:

- [Docker](<https://en.wikipedia.org/wiki/Docker_(software)>), which is a
  popular yet heavy-weight container mechanism. Docker runs Linux containers,
  but it runs on Mac and Windows as well by running a Linux VM. Under Docker,
  the environments may use sudo to modify their root partitions. See the
  [Docker-specific docs](docs/Docker.md) for details, including security
  implications and installation instructions.

- [Bubblewrap](https://github.com/containers/bubblewrap), which is a
  light-weight mechanism that runs on Linux only. Under Bubblewrap, the host's
  root partition is shared read-only with the environments. See the
  [Bubblewrap-specific docs](docs/Bubblewrap.md) for details, including
  security implications and installation instructions.

- System user accounts, created and switched to via `sudo`. With system user
  accounts, the operating system prevents (or not) the environments from
  reading/writing the root partition and other user's files with classical file
  permissions. See the [User accounts-specific docs](docs/User.md) for details,
  including security implications and installation instructions.

Cubicle is in early stages of development and is likely to change frequently in
incompatible ways. Users should review the Git commits to see what's changed
before upgrading.

## Motivation

Cubicle protects your host system, protects development projects from
interfering with each other, and lets you take advantage of modern developer
tools.

Modern development requires running a lot of other people's code. First,
there's tooling, including editors/IDEs, compilers, code formatters, linters,
test runners, bundlers, and package managers, and these often differ from one
language/ecosystem to another. These tools and your code's increasing number of
dependencies may be subject to software [supply chain
attacks](https://en.wikipedia.org/wiki/Supply_chain_attack), where a regular
software update suddenly gives an attacker control. It's impractical to audit
all the code you run to develop software, and for many projects, it's even
impractical to audit all your software's own dependencies in depth.

I found that I was avoiding lots of tools just because I didn't want to add
risk to my host system. Do I want to install that janky VS Code extension that
has 3 users, just for syntax highlighting? When it suggests installing a
linter, do I want to do that too? (Yes, there's some irony in that Cubicle
itself is a janky program that has fewer than 3 users -- for now.)

Modern development also moves fast. VS Code gets updates every month. Rust
nightly builds are updated, well, nightly. It's hard for distributions to keep
up, so you probably end up downloading and maintaining a lot of key tools in
userspace.

With Cubicle, you can have a pristine, isolated development environment with
modern tooling every single time. If you learn about some new open source
project through the bird app or the orange website or whatever, Cubicle lets
you spin up a new environment in seconds to try things out. When you find
something you like, you can define a package so that it's always ready to go
and up to date in your environments.

### What does this provide over Docker?

[Docker](<https://en.wikipedia.org/wiki/Docker_(software)>) is a popular
container manager. It's commonly used to run long-running network services
(daemons) in an isolated an reproducible environment. It's also used to build
(compile and package) software in a reproducible environment. Docker is very
flexible. It's less commonly used this way, but you can develop software inside
of a Docker container, or share your source files between your host and a
container, running your editor on the host and the other tools inside the
container.

Docker containers are usually immutable and built in sequential layers. They're
used for the purpose of running a build or a single version of a service, then
they are thrown away and replaced.

Cubicle packages are built independently and then mixed together to populate a
container environment. Cubicle promotes developer experimentation, so
containers can be longer lived and modified in place, if desired. It's easy and
common to replace the guts of a Cubicle container while maintaining the user's
work.

## Feedback

This project is still in early stages. It feels like I've stumbled on a better
way of doing things, but I don't know what this project should be when it grows
up. I've shared this for the purpose of gathering feedback from early users.
Please leave feedback in the GitHub Discussions.

## Security

The goal of any sandbox is to isolate software so that software running inside
the sandbox cannot "break out", meaning it cannot access or affect the system
outside the sandbox. Cubicle may not meet this goal, at least depending on the
environment and the enforcement mechanism used. Cubicle does offer a meaningful
layer of security when compared to running untrusted software directly on your
host.

## Cubicle Packages

Since Cubicle environments are created and recreated often, it's helpful to
inject configuration and program files into them. This allows you to use a
new container right away and not grow attached to them.

The current package format is pretty simple. A package definition is named
after its directory. It must contain one or more of these files:

- `update.sh`: An executable that is run in a package builder environment to
  produce the package files. The script may download software, unpack it,
  compile it, set up configuration files, etc. It should create an archive at
  `~/provides.tar` that Cubicle will later unpack in the target environments'
  home directories. Note that `update.sh` runs when the package builder
  environment is first created and also when the package builder environment is
  updated. The package builder environments are kept around as a form of
  caching.

- `build-depends.txt`: A newline-separated list of package names on which this
  package depends when it is built. The package builder environment will be
  seeded with the listed packages, but other environments that depend on this
  package will not.

- `depends.txt`: A newline-separated list of package names on which this
  package or its output depends. Both the package builder environment and the
  new environments that depend on this package will be seeded with the listed
  packages.

- `test.sh`: An executable that is run in a clean environment to sanity check
  the package output files. The test environment is seeded with the package's
  dependencies, the package output files, and the package source directory
  (except `update.sh`).

These files and any other files in the package directory are injected into the
work directory of the package builder environment.

If the package provides any executable files within `~/.dev-init/`, these will
be run upon creating and resetting target environments.

Packages are built automatically when they are first used, and they are updated
when they are used if:

- 12 hours have elapsed (to change this, set `auto_update` to `never`, `1h`,
  `3.5 days`, etc in the configuration file),
- their package definitions have changed, or
- one of their dependencies or build-dependencies has been updated more
  recently.

Cubicle looks for package definitions in the following locations:

1. Local packages in `${XDG_DATA_HOME:-~/.local/share}/cubicle/packages/*/`.
2. Built-in packages in the Cubicle source code's `packages/` directory.

If a package with the same name appears in multiple locations, the first one is
used and the others are ignored. The sort order of the names of the containing
directories is significant for local packages, so you may want to create
`00local` to come first.

The package named "default" is used for new environments when a package list is
not otherwise specified.

The package named "auto" and its dependencies are automatically included in
every environment except those that "auto" itself transitively depends upon.
This is useful for configuration files, for example.

### Related Projects

- [Bubblewrap](https://github.com/containers/bubblewrap) is a low-level tool
  from the Flatpak developers to run lightweight containers. Julia Evans wrote a
  recent
  [blog post exploring Bubblewrap](https://jvns.ca/blog/2022/06/28/some-notes-on-bubblewrap/).
- [Development Containers](https://containers.dev/) appears to be a proposed
  specification by Microsoft for configuring full-featured development
  environments using Docker.
- [Docker](<https://en.wikipedia.org/wiki/Docker_(software)>) is a popular
  container manager.
- [Firejail](https://github.com/netblue30/firejail) defines and enforces
  per-application profiles to sandbox programs and GUI applications.
- [Flatpak](https://en.wikipedia.org/wiki/Flatpak) runs packaged GUI
  applications in a sandbox. Bubblewrap came out of the Flatpak project and is
  used by Flatpak.
- [LXC](https://linuxcontainers.org/lxc/introduction/) is a low-level tool for
  running large, long-lived Linux containers, like VMs but with less overhead.
- [LXD](https://linuxcontainers.org/lxd/introduction/) is a command-line tool
  to manage LXC containers (and VMs).
- [Nix](<https://en.wikipedia.org/wiki/Nix_(package_manager)>) is a package
  manager that installs software in isolated directories. This isolates
  software installations from each other and allows multiple versions of
  packages to coexist. It does not appear to prevent packages from interacting
  through files in the user's home directory.
- [Podman](https://github.com/containers/podman) is simlar to Docker but
  daemonless. Its CLI is mostly compatible with Docker's.
- [Vagrant](https://www.vagrantup.com/) is a tool for automatically configuring
  virtual machines and Docker containers. It uses so-called "Provisioners" like
  shell scripts, Puppet, or Chef to configure the environments.

### Related Articles

- [Docker Containers on the Desktop](https://blog.jessfraz.com/post/docker-containers-on-the-desktop/)
  by Jessie Frazelle (2015). Discusses using Docker to isolate desktop apps,
  including 10 examples.
