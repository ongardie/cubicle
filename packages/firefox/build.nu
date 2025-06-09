#!/usr/bin/env nu

use std/assert

print "Checking latest Firefox version"

let url = (
    http get --full --redirect-mode manual 'https://download.mozilla.org/?product=firefox-latest-ssl&os=linux64&lang=en-US'
    | get headers.response
    | transpose -dr
    | get location
)
let version = $url | parse -r '/firefox-(?<version>.*).tar.xz$' | get version.0

let have = try {
    firefox --version
} catch {
    "none"
}
if $have == $"Mozilla Firefox ($version)" {
    print $"Firefox ($version) already installed"
} else {
    print $"Downloading Firefox ($version)"
    mkdir ~/opt
    rm -rf ~/opt/firefox/
    http get $url | tar -C ~/opt -x --xz
    ln -fs ../opt/firefox/firefox ~/bin/firefox
    assert equal (firefox --version) $"Mozilla Firefox ($version)"
}

if ("~/.mozilla/firefox/profiles.ini" | path exists) == false {
    print "Initializing Firefox profile"
    firefox --first-startup --screenshot out+err> /dev/null
    let profile = glob ~/.mozilla/firefox/*.default-release | first
    '
        user_pref("browser.aboutConfig.showWarning", false);
        user_pref("browser.aboutwelcome.enabled", false);
        user_pref("browser.contentblocking.category", "strict");
        user_pref("browser.startup.homepage_welcome_url", "");
        user_pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);
        user_pref("dom.security.https_only_mode", true);
        user_pref("dom.security.https_only_mode_ever_enabled", true);
        user_pref("extensions.formautofill.addresses.enabled", false);
        user_pref("extensions.formautofill.creditCards.enabled", false);
        user_pref("network.cookie.cookieBehavior", 5);
        user_pref("privacy.annotate_channels.strict_list.enabled", true);
        user_pref("privacy.donottrackheader.enabled", true);
        user_pref("privacy.trackingprotection.enabled", true);
        user_pref("privacy.trackingprotection.socialtracking.enabled", true);
        user_pref("trailhead.firstrun.branches", "nofirstrun-empty");
        user_pref("trailhead.firstrun.didSeeAboutWelcome", true);
    ' | save -a ($profile | path join prefs.js)
}

mkdir ~/.local/share/applications/
cp firefox.desktop ~/.local/share/applications/

'# This writes to `~/.config/mimeapps.list`.
xdg-mime default firefox.desktop x-scheme-handler/https x-scheme-handler/http
' | save -f ~/.dev-init/firefox.sh
chmod +x ~/.dev-init/firefox.sh

'export BROWSER=firefox' | save -f ~/.config/profile.d/60-browser.sh

tar -c -C ~ --verbatim-files-from --files-from ~/w/provides.txt -f ~/provides.tar
