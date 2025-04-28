#!/bin/sh

set -eu

shellcheck "$@" \
    packages/configs-core/dot-bash_profile \
    packages/configs-core/dot-bashrc \
    packages/configs-core/dot-profile \
    packages/vscodium/codium

find . -type f \( -name '*.sh' -or -name '*.bash' \) -exec shellcheck "$@" {} \+
