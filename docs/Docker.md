# Cubicle Docker Runner

This document describes using Cubicle using the `docker` runner.
[Docker](<https://en.wikipedia.org/wiki/Docker_(software)>) is a popular yet
heavy-weight container mechanism. Docker runs Linux containers, but it runs on
Mac and Windows as well by running a Linux VM. Under Docker, the environments
may use sudo to modify their root partitions.

## Security

Cubicle relies on Docker and the Linux kernel for isolation, which aren't
perfect. Users should review Docker's security and, of course, keep up with
Linux kernel updates.

Environments with access to X11 probably have full access to your keystrokes.
See <https://wiki.archlinux.org/title/Bubblewrap#Sandboxing_X11> for more info.

Under Docker, Cubicle uses the default network configuration, which isolates
the containers in their own network namespace.

Under Docker, Cubicle uses the default resource limits. This may leave
containers vulnerable to attacks like unauthorized cryptocurrency mining.

## Installation

Cubicle is made up of a Rust program that runs on the host and a collection of
shell scripts for package setup that run in containers.

### Installing Dependencies

[Install Docker](https://docs.docker.com/get-docker/). On Debian 12, you can
install it using `apt`:

```sh
sudo apt install docker.io
```

### Installing Cubicle

Assuming you'd like to install into `~/opt/cubicle` and already have `~/bin` in
your `$PATH`:

```sh
echo 'runner = "docker"' > ~/.config/cubicle.toml
cd ~/opt/
git clone https://github.com/ongardie/cubicle/
cd cubicle
ln -s $(pwd)/target/release/cub ~/bin/cub
```

Now, if you have a recent version of [Rust and
Cargo](https://www.rust-lang.org/tools/install) installed, you can run:

```sh
cargo build --release
```

Alternatively, you can use Docker to run the Rust compiler, if the host system
is similar enough to Debian 12 (Bookworm):

```sh
docker run --rm \
    --user "$(id -u):$(id -g)" \
    -v "$PWD:$PWD" -w "$PWD" \
    rust:1-bookworm \
    cargo build --release
```

### Installing a Seccomp Filter

Bublewrap's security depends on setting a restrictive
[seccomp](https://en.wikipedia.org/wiki/Seccomp) policy, to limit the system
calls available to the sandbox environment.

Docker's default filter doesn't allow VS Codium, other Electron
apps, or Chromium to run with the Chromium sandbox enabled: see
<https://github.com/moby/moby/issues/42441> and
<https://chromium.googlesource.com/chromium/src/+/HEAD/docs/linux/sandboxing.md>.

To work around this, we can edit Docker's seccomp policy to allow `clone` and
`unshare` unconditionally (which adds risk):

```sh
curl -L 'https://raw.githubusercontent.com/moby/moby/master/profiles/seccomp/default.json' > docker-seccomp.json
sed 's/"getpid",/"getpid", "clone", "unshare",/' < docker-seccomp.json > seccomp.json
```

Then, can point Cubicle to the `seccomp.json` file using the `seccomp`
configuration option below.

## Configuration

Inside your `cubicle.toml`, set `runner` to `"docker"`. You can optionally
create an object named `docker` with the following keys:

### `bind_mounts`

- Type: boolean
- Default: `false`

If false (default), the Docker runner will use volume mounts for the
environments' home and work directories.

If true, the runner will use bind mounts instead. Bind mounts are probably only
advantageous on Linux; they can be more convenient because they can be owned by
the normal user on the host.

### `prefix`

- Type: string
- Default: `"cub-"`

This string is prepended to all the Docker object names (container, image, and
volume names) that the Cubicle runner creates. It defaults to "cub-". Using the
empty string is also allowed.

### `seccomp`

- Type: path or none
- Default: none

If set, Cubicle will use this JSON-formatted seccomp filter with Docker.
Otherwise, Cubicle will use Docker's default seccomp filter. See the seccomp
discussion above for more information.

### `strict_debian_packages`

- Type: boolean
- Default: `false`

If false (default), the Docker runner will use a base image with a larger
collection of Debian packages already installed. This currently builds an image
with every Debian package mentioned by any visible Cubicle package (and assumes
that none of these conflict). After this image is built, it can be reused many
times, making this the faster option.

If true, the Docker runner will use a minimal base image and will install the
strictly needed set of Debian packages within each container. This will be
slower overall, but it's useful when developing packages to ensure that a
Cubicle package can build with only its explicitly declared set of Debian
package dependencies. It's also useful in the CI environment to avoid building
a large base image that will go largely unused.

## Uninstalling

First, exit out of any running Cubicle environments.

Using `docker rm --force`, stop and remove the running Cubicle containers.

Then, run:

```sh
docker rmi cubicle-base
```

Assuming the same paths as in the installation instructions above:

```sh
rm -r ~/opt/cubicle/
rm ~/bin/cub
```

You may also want to remove these directories if you're done with all your
Cubicle environments:

```sh
rm -r ${XDG_CACHE_HOME:-~/.cache}/cubicle/
rm -r ${XDG_DATA_HOME:-~/.local/share}/cubicle/
```

## Cubicle Environments

Each Cubicle environment consists of three logical filesystem layers:

| Layer   | Storage (with default config) | Container Path   | Lifetime |
| ------- | ----------------------------- | ---------------- | -------- |
| 1. OS   | cub-cubicle-base Docker image | `/` (read-write) | short    |
| 2. home | cub-ENV-home Docker volume    | `~/`             | short    |
| 3. work | cub-ENV-work Docker volume    | `~/w/`           | long     |

1. The base operating system. This is the "cub-cubicle-base" Docker image that
   is built automatically by Cubicle. It's currently based on Debian 12.

2. A home directory. Inside the environment, this is at the same path as the
   host's `$HOME`, but it's not shared with the host. It lives in
   `${XDG_CACHE_HOME:-~/.cache}/cubicle/home/` on the host with bind mounts
   or in a `cub-ENV-home` Docker volume with volume mounts.
   The home directory
   should be treated as replaceable at any time. Cubicle populates the home
   directory with files from packages when you create the environment (with
   `cub new`) or reset it (with `cub reset`). Currently, the home directory is
   populated with physical copies of package files, so the home directories can
   be large (a few gigabytes) and can take a few seconds to initialize.

3. A work directory. his is at `~/w/` inside the environment. For an
   environment named `eee`, this is at
   `${XDG_DATA_HOME:-~/.local/share}/cubicle/work/eee/` on the host with bind
   mounts or in a `cub-eee-home` Docker volume with volume mounts. The work
   directory is where any important files should go. It persists across
   `cub reset`.

There are a couple of special files in the work directory:

- An executable placed at `~/w/update.sh` will be run automatically at the end
  of `cub reset`. This can be a useful hook to re-configure a new home
  directory.

- A file named `~/w/packages.txt` keeps track of which packages the environment
  was initialized or last reset with. It is used next time the environment is
  reset (unless the user overrides that on the command line).
