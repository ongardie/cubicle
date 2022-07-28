# Cubicle development container manager

Cubicle is a program to manage light-weight containers or sandbox environments.
It is intended for isolating development environments from the host system and
from each other.

Cubicle currently runs on Linux only, as it is built on
[Bubblewrap](https://github.com/containers/bubblewrap). Cubicle is in early
stages of development and is likely to change frequently in incompatible ways.
Users should review the Git commits to see what's changed before upgrading.

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
linter, do I want to do that too?

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

### Why Not Docker?

[Docker](<https://en.wikipedia.org/wiki/Docker_(software)>) is a popular
container manager. It's commonly used to run long-running network services
(daemons) in an isolated an reproducible environment. It's also used to build
(compile and package) software in a reproducible environment. Docker is very
flexible. It's less commonly used this way, but you can develop software inside
of a Docker container, or share your source files between your host and a
container, running your editor on the host and the other tools inside the
container.

Cubicle could probably be rebuilt on top of Docker, but the two don't quite
align on how containers should be populated or used. For example, Docker
containers are usually immutable. They are built in sequential layers. They're
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
environment. Cubicle does offer a meaningful layer of security when compared to
running untrusted software directly on your host.

Cubicle builds upon Bubblewrap and the Linux kernel, which aren't perfect.
Users should review Bubblewrap's security and, of course, keep up with Linux
kernel updates.

Of particular note, containers with access to X11 probably have full access to
your keystrokes. See https://wiki.archlinux.org/title/Bubblewrap#Sandboxing_X11
for more info.

Cubicle does not currently limit host network access, allowing containers to
access services on the local host and local network.

Cubicle does not currently limit the resources used by its containers. This may
leave containers vulnerable to attacks like unauthorized cryptocurrency mining.

### Seccomp Filter

Cubicle and Bublewrap's security depend on setting a restrictive
[seccomp](https://en.wikipedia.org/wiki/Seccomp) policy, to limit the system
calls available to the sandbox environment. Developing such a policy requires a
careful audit of what is safe or unsafe to expose in the Linux kernel, which is
a moving target. Cubicle does not currently ship such a seccomp filter. Users
are encouraged to borrow one from these projects:

- Podman/Buildah/CRI-O's seccomp filter is here:
  <https://github.com/containers/common/blob/main/pkg/seccomp/seccomp.json>.
  This filter does allow running Electron apps with the Chromium sandbox.

- Docker's seccomp filter is in
  [moby/profiles/seccomp](https://github.com/moby/moby/tree/master/profiles/seccomp)
  in both JSON format and Golang. This filter doesn’t allow running Electron
  apps like VS Code with the Chromium Sandbox turned on. See
  <https://github.com/moby/moby/issues/42441> for details.

- Flatpak's seccomp filter is around here:
  [common/flatpak-run.c](https://github.com/flatpak/flatpak/blob/main/common/flatpak-run.c#L3073)
  in `setup_seccomp`. I haven't tried this one with Cubicle.

This [short C
program](https://github.com/bradfa/tlpi-dist/blob/master/seccomp/dump_seccomp_filter.c)
can dump a compiled BFP seccomp filter that is installed in a running process.
It uses
[`PTRACE_SECCOMP_GET_FILTER`](https://manpages.debian.org/bullseye/manpages-dev/ptrace.2.en.html#PTRACE_SECCOMP_GET_FILTER)
to do this. Extracting a filter this way can be easier than trying to compile a
BPF filter from the above source code.

## Installing

Cubicle is currently a single-file Python script (that uses no third-party
libraries) and a collection of shell scripts. Installation is straightforward.

### Installing Dependencies

First, install the dependencies:

- `bwrap` - Bubblewrap, Linux light-weight container tool. Note that while
  bwrap used to be a setuid binary, this is no longer needed on modern
  distributions.
- `curl` - HTTP client.
- `git` - version control system.
- `jq` - command-line JSON processor.
- `pv` - pipe viewer, displays progress bars.

You also need Python 3 and `tar`, but you probably already have those.

On Debian 11, you can install the dependencies using `apt`:

```sh
sudo apt install bubblewrap curl git jq pv
```

### Installing Cubicle

Assuming you'd like to install into `~/opt/cubicle` and already have `~/bin` in
your `$PATH`:

```sh
cd ~/opt/
git clone https://github.com/ongardie/cubicle/
cd cubicle
ln -s $(pwd)/cubicle.py ~/bin/cub
```

### Installing a Seccomp Filter

If you haven't done so already, please read the security section on why you
need a good seccomp filter. I've extracted a filter from a running Podman
container on my amd64 machine and shared it for convenience. I don't know
whether this is secure (for any sandboxing purpose) or will remain so over
time. Podman is released under the Apache-2.0 license.

```sh
curl -LO 'https://ongardie.net/static/podman.bpf'
ln -s podman.bpf seccomp.bpf
```

### Installing Shell Completions

If you use Zsh ([Z shell](https://en.wikipedia.org/wiki/Z_shell)), you can set
up shell completions. You can check `$fpath` to see where this should go. This
example assumes `~/.zfunc` is listed in `$fpath` already:

```sh
ln -s $(pwd)/_cub ~/.zfunc/
```

### Uninstalling

First, exit out of any running Cubicle environments.

Assuming the same paths as above:

```sh
rm -r ~/opt/cubicle/
rm ~/bin/cub
rm ~/.zfunc/_cub
```

You may also want to remove these directories if you're done with all your
Cubicle environments:

```sh
rm -r ${XDG_CACHE_HOME:-~/.cache}/cubicle/
rm -r ${XDG_DATA_HOME:-~/.local/share}/cubicle/
```

## Cubicle Environments

Each Cubicle environment consists of three logical filesystem layers:

| Layer   | Host Path (with default XDG base dirs) | Container Path  | Lifetime |
| ------- | -------------------------------------- | --------------- | -------- |
| 1. OS   | `/`                                    | `/` (read-only) | long     |
| 2. home | `~/.cache/cubicle/home/ENV`            | `~/`            | short    |
| 3. work | `~/.local/share/cubicle/work/ENV`      | `~/ENV/`        | long     |

1. The base operating system. This is currently shared with the host's `/` and
   read-only inside the container.

2. A home directory. Inside the environment, this is at the same path as the
   host's `$HOME`, but it's not shared with the host. It lives in
   `${XDG_CACHE_HOME:-~/.cache}/cubicle/home/` on the host. The home directory
   should be treated as replaceable at any time. Cubicle populates the home
   directory with files from packages when you create the environment (with
   `cub new`) or reset it (with `cub reset`). Currently, the home directory is
   populated with physical copies of package files, so the home directories can
   be large (a few gigabytes) and can take a few seconds to initialize.

3. A work directory. For an environment named `x`, this is at `~/x` inside the
   environment and `${XDG_DATA_HOME:-~/.local/share}/cubicle/work/x/` on the
   host. An environment variable named `$SANDBOX` is automatically set to the
   name of the environment and can be used to access the work directory
   conveniently from scripts (as `~/$SANDBOX/`). The work directory is where
   any important files should go. It persists across `cub reset`.

There are a couple of special files in the work directory:

- An executable placed at `~/$SANDBOX/update.sh` will be run automatically at
  the end of `cub reset`. This can be a useful hook to re-configure a new home
  directory.

- A file named `~/$SANDBOX/packages.txt` keeps track of which packages the
  environment was initialized or last reset with. It is used next time the
  environment is reset (unless the user overrides that on the command line).

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

- `depends.txt`: A newline-separated list of package names on which this
  package depends. Both the package builder environment and the new
  environments that depend on this package will be seeded with these packages.

These files and any other files in the package directory are injected into the
work directory of the package builder environment.

If the package provides any executable files within `~/.dev-init/`, these will
be run upon creating and resetting target environments.

Packages are built automatically when they are first used, and they are updated
when they are used if 12 hours have elapsed, their package definitions have
changed, or one of their dependencies has been updated more recently.

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
  from the Flatpak developers to run lightweight containers. Cubicle is
  currently a wrapper for Bubblewrap. Julia Evans wrote a recent
  [blog post exploring Bubblewrap](https://jvns.ca/blog/2022/06/28/some-notes-on-bubblewrap/).
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
- [Podman](https://github.com/containers/podman) is simlar to Docker but
  daemonless. Its CLI is mostly compatible with Docker's.
