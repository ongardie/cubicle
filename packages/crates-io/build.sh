#!/bin/sh
set -eu
cd

if ! [ -e exclude.txt ]; then
    find .cargo/bin/* > exclude.txt
fi

arch="$(uname -m)"
set --
case "$PACKAGE" in
    cargo-audit)
        # Set targets to prefer musl because the glibc builds expect 2.39 but
        # Debian 12 has 2.36. Set pkg-url because binstall won't find it.
        set -- \
            --targets "$arch-unknown-linux-musl" \
            --targets "$arch-unknown-linux-gnu" \
            --pkg-url '{repo}/releases/download/{name}/v{version}/{name}-{target}-v{version}{archive-suffix}'
        ;;

    tealdeer)
        set -- \
            --pkg-fmt bin \
            --pkg-url '{repo}/releases/download/v{version}/{name}-{target-family}-{target-arch}-{target-libc}'
        ;;
esac

cargo binstall --no-confirm --force "$@" "$PACKAGE"

# shellcheck disable=SC2016
echo '$HOME/.cargo/bin' > .config/profile.d/path/33-cargo

tar --create --file provides.tar --exclude-from exclude.txt .cargo/bin .config/profile.d/path/33-cargo
