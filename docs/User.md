# Cubicle User Runner

This document describes using Cubicle using the `user` runner. This runner uses
system user accounts, created and switched to via `sudo`. With system user
accounts, the operating system prevents (or not) the environments from
reading/writing the root partition and other user's files with classical file
permissions.

One challenge with the user account runner is that packages are built at one
path, such as `/home/cub-package-python/opt/python`, but then unpacked and used
at another path, such as `/home/cub-dev/opt/python`. Not all packages are
portable or relocatable like this.

Another challenge is that, in principle, this runner could work on a variety of
operating systems. In practice, packages may not be compatible with operating
systems that differ too much from Debian stable.

## Security

Cubicle relies on the Linux kernel for isolation, which isn't perfect. Users
should keep up with Linux kernel updates.

Cubicle uses `sudo` to create and destroy user accounts and to switch users.
Allowing Cubicle to execute such commands opens up the host system to attack.

For the system to prevent users from accessing each other's files, they must
set appropriate filesystem permissions. A `umask` setting of at least `007` is
recommended for every user. Additionally, it's a good idea to scan for
mistakes periodically:

```sh
sudo find /root /home -perm /o=rwx -not -type l
```

Environments with access to X11 probably have full access to your keystrokes.
See <https://wiki.archlinux.org/title/Bubblewrap#Sandboxing_X11> for more info.
To grant X11 access anyway to different user accounts, you may need to set a
looser policy with `xhost`.

With system user accounts, Cubicle does not limit host network access, allowing
environments to access services on the local host and local network. The UNIX
domain abstract socket namespace is also shared between the host and the
containers, since it is also tied to the network namespace.

With system user accounts, Cubicle does not enforce any resource limits. This
may leave containers vulnerable to attacks like unauthorized cryptocurrency
mining.

## Installation

Cubicle is made up of a Rust program that runs on the host and a collection of
shell scripts for package setup that run in containers.

### Installing Dependencies

For now, you'll need a Debian-based system with `sudo`, and `adduser`.
Otherwise, follow the same instructions as for Bubblewrap: see
<docs/Bubblewrap.md>.

### Installing Cubicle

Assuming you'd like to install into `~/opt/cubicle` and already have `~/bin` in
your `$PATH`:

```sh
echo 'runner = "user"' > ~/.config/cubicle.toml
cd ~/opt/
git clone https://github.com/ongardie/cubicle/
cd cubicle
cargo build --release
ln -s $(pwd)/target/release/cubicle ~/bin/cub
```

## Uninstalling

First, exit out of any running Cubicle environments.

Run `grep ^cub- /etc/passwd` to list out your user accounts. Then, remove each
account and its files:

```sh
sudo deluser --removehome $ACCOUNT
```

Assuming the same paths as in the installation instructions above:

```sh
rm -r ~/opt/cubicle/
rm ~/bin/cub
```

## Cubicle Environments

Each Cubicle environment consists of a new system user account. The Cubicle
environment names are prefixed with `cub-` to avoid collisions with regular
user accounts.

The user account's home directory should be treated as replaceable at any time.
Cubicle populates the home directory with files from packages when you create
the environment (with `cub new`) or reset it (with `cub reset`). Currently, the
home directory is populated with physical copies of package files, so the home
directories can be large (a few gigabytes) and can take a few seconds to
initialize.

Inside the home directory is a work directory at `~/w/`. The work directory is
where any important files should go. It persists across `cub reset`.

There are a couple of special files in the work directory:

- An executable placed at `~/w/update.sh` will be run automatically at the end
  of `cub reset`. This can be a useful hook to re-configure a new home
  directory.

- A file named `~/w/packages.txt` keeps track of which packages the environment
  was initialized or last reset with. It is used next time the environment is
  reset (unless the user overrides that on the command line).
