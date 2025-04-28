#!/usr/bin/env nu

# This searches for available APT packages that contain an executable with the
# given name. This is similar to `apt-file search "bin/$1 "`, but this is
# faster at searching and provides nicer output. This is also similar to
# `command-not-found`, but this does not require a precomputed index.
#
# There's a related blog post explaining a Posix shell precursor to the
# implementation <https://ongardie.net/blog/command-not-found/>, and a post
# discussing this port to Nushell <https://ongardie.net/blog/nushell/>.

# Find APT packages that contain an executable with the given name.
def main [
    command: string # Executable name
]: nothing -> nothing {
    let files = glob /var/lib/apt/lists/*Contents* --exclude [*.diff_Index]
    if ($files | is-empty) {
        print "Run 'sudo apt install apt-file; sudo apt update' to search package contents."
        return
    }

    # The lines contain one or multiple packages, like this:
    # usr/bin/parallel                  utils/parallel
    # usr/bin/parallel                  utils/moreutils,utils/parallel
    let packages = $files
        | par-each {
            /usr/lib/apt/apt-helper cat-file $in
                | parse -r ('^(?:usr/)?s?bin/' + $command + '[ \t]+(?<packages>.*)$')
                | get packages
                | split row ','
                | str replace -r '^.*/' ''
        }
        | flatten
        | uniq
    if ($packages | is-empty) {
        print "No packages found"
        return
    }

    # Instead of printing the package names, this prints a brief description
    # and information about versions. You're not supposed to use 'apt' in
    # scripts, but it's hard to get this concise output any other way.
    apt search --names-only ('^(' + ($packages | str join '|') + ')$')
}
