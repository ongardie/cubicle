#!/bin/sh

# If $HOME contained '&', '\1'-'\9', or '|', that would throw off the sed
# replacement below.
case "$HOME" in
    *'&'* | *"\\"* | *'|'*)
        # shellcheck disable=SC2016
        echo 'WARNING: $HOME contains weird characters. Not setting $PATH.' >&2
        ;;
    *)
        PATH=$(
            sed -e "s/#.*//; s|\$HOME|$HOME|g" ~/.config/profile.d/path/* | \
            paste -sd: | \
            sed 's/:\+/:/g; s/:$//'
        )
        export PATH
        ;;
esac
