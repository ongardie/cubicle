#!/bin/sh

# This searches for available APT packages that contain an executable with the
# given name. This is similar to `apt-file search "bin/$1 "`, but this is
# faster at searching and provides nicer output. This is also similar to
# `command-not-found`, but this does not require a precomputed index.
#
# There's a blog post explaining the motivation and implementation here:
# <https://ongardie.net/blog/command-not-found/>.

set -eu

if [ $# -ne 1 ]; then
    echo "Find APT packages that contain an executable with the given name."
    echo "Usage: $(basename "$0") PROGRAM" >&2
    exit 1
fi

files() {
    find /var/lib/apt/lists/ -maxdepth 1 -name '*Contents*' -not -name '*.diff_Index' -print0
}
FILES="$(files)" # without null delimiters
if [ -z "$FILES" ]; then
    echo "Run 'sudo apt install apt-file && sudo apt update' to search package contents."
    exit 1
fi

PATTERN="^(usr/)?s?bin/$1\s"
if command -v rg > /dev/null && command -v lz4 > /dev/null; then
    LINES=$(files | xargs -0 rg --no-filename --search-zip "$PATTERN")
else
    echo "Run 'sudo apt install ripgrep lz4' to speed this up"
    LINES=$(files | xargs -0 /usr/lib/apt/apt-helper cat-file | grep -P "$PATTERN")
fi

# The lines contain one or multiple packages, like this:
# usr/bin/parallel                  utils/parallel
# usr/bin/parallel                  utils/moreutils,utils/parallel
#
# This sed expression drops the filename, splits the package list by the comma
# delimiter, and drops the section names.
PACKAGES=$(echo "$LINES" | sed -E 's/^.* +//; s/,/\n/g; s/^.*\///m' | sort -u)

if [ -z "$PACKAGES" ]; then
    echo "No packages found"
    exit
fi

# Instead of printing the package names, this prints a brief description and
# information about versions. You're not supposed to use 'apt' in scripts, but
# it's hard to get this concise output any other way.
PACKAGES_DISJUNCTION=$(echo "$PACKAGES" | paste -s -d '|')
apt search --names-only "^($PACKAGES_DISJUNCTION)$"
