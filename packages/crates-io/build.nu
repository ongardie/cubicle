#!/usr/bin/env nu

cd

if ("exclude.txt" | path exists) == false {
    glob .cargo/bin/* | path relative-to ~ | save exclude.txt
}

let arch = uname | get machine

let args = match $env.PACKAGE {
    "cargo-audit" => [
        # Set targets to prefer musl because the glibc builds expect 2.39 but
        # Debian 12 has 2.36. Set pkg-url because binstall won't find it.
        --targets $"($arch)-unknown-linux-musl"
        --targets $"($arch)-unknown-linux-gnu"
        --pkg-url '{repo}/releases/download/{name}/v{version}/{name}-{target}-v{version}{archive-suffix}'
    ],

    "tealdeer" => [
        --pkg-fmt bin
        --pkg-url '{repo}/releases/download/v{version}/{name}-{target-family}-{target-arch}-{target-libc}'
    ],

    _ => [],
}

cargo binstall --no-confirm --force ...$args $env.PACKAGE

'$HOME/.cargo/bin' | save -f .config/profile.d/path/33-cargo

tar --create --file provides.tar --exclude-from exclude.txt .cargo/bin .config/profile.d/path/33-cargo
