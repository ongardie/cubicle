PATH=$(sed -e "s|\$HOME|$HOME|" ~/.config/profile.d/path/* | sed -z 's/\n/:/g;s/:$/\n/')
export PATH
