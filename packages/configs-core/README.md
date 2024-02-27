# configs-core package

This package contains a structure for environment and shell configurations, and
it provides some very basic configurations. This is used for both interactive
Cubicle environments (via the
[configs-interactive](../configs-interactive/README.md) package) and for
package builder and test environments (via the
[auto-batch](../auto-batch/README.md) package).

## Overview of files

Once per login -- set environment variables:

- **[`~/.profile`](./dot-profile):** Sourced at login by
  [`~/.bash_profile`](./dot-bash_profile) and [`~/.zprofile`](./dot-zprofile).
  It is POSIX-compatible and sources the scripts in
  [`~/.config/profile.d/*.sh`](./profile.d/).
- **[`~/.bash_profile`](./dot-bash_profile):** Bash sources this for interactive
  login shells. It sources [`~/.profile`](./dot-profile) and then
  [`~/.bashrc`](./dot-bashrc).
- **[`~/.zprofile`](./dot-zprofile):** Zsh sources this for interactive login
  shells (before [`~/.zshrc`](./dot-zshrc)). It sources
  [`~/.profile`](./dot-profile).

Once per shell -- set aliases, define functions, and configure shells:

- **[`~/.bashrc`](./dot-bashrc):** Bash sources this for interactive non-login
  shells, and [`~/.bash_profile`](./dot-bash_profile) sources this for
  interactive login shells. It sources the POSIX-compatible scripts in
  [`~/.config/shrc.d/*.sh`](./shrc.d/) and then the Bash-specific
  scripts in [`~/.config/bashrc.d/*.bash`](./bashrc.d/).
- **[`~/.zshrc`](./dot-zshrc):** Zsh sources this for interactive (login and
  non-login) shells. It sources the POSIX-compatible scripts in
  [`~/.config/shrc.d/*.sh`](./shrc.d/) and then the Zsh-specific
  scripts in [`~/.config/zshrc.d/*.zsh`](./zshrc.d/).

This is a useful reference:
<https://shreevatsa.wordpress.com/2008/03/30/zshbash-startup-files-loading-order-bashrc-zshrc-etc/>.
