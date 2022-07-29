#!/bin/sh
set -e

echo "Checking latest Firefox version"
url=$(curl 'https://download.mozilla.org/?product=firefox-latest-ssl&os=linux64&lang=en-US' -w '%{redirect_url}\n' -s --output /dev/null)
version=$(basename "$url" | sed -E 's/^firefox-(.*).tar.bz2$/\1/')
if [ "$(firefox --version)" = "Mozilla Firefox $version" ]; then
    echo "Firefox $version already installed"
else
    echo "Downloading Firefox $version"
    mkdir -p ~/opt
    rm -rf ~/opt/firefox/
    curl -L "$url" | tar -C ~/opt -xj
    ln -fs ~/opt/firefox/firefox ~/bin/firefox
    firefox --version
fi

if [ ! -f ~/.mozilla/firefox/profiles.ini ]; then
    echo "Initializing Firefox profile"
    firefox --screenshot 2>/dev/null
    profile=$(echo ~/.mozilla/firefox/*.default-release)
    cat >> $profile/prefs.js <<"EOF"
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
EOF
fi

cat > ~/.dev-init/firefox.sh <<"EOF"
# This writes to `~/.config/mimeapps.list`.
xdg-mime default firefox-esr.desktop x-scheme-handler/https x-scheme-handler/http
EOF
chmod +x ~/.dev-init/firefox.sh

tar -c -C ~ --verbatim-files-from --files-from ~/$SANDBOX/provides.txt -f ~/provides.tar
