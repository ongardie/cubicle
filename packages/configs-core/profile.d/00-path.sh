PATH=$(sed -e "s|\$HOME|$HOME|" ~/.configs/profile.d/path/* | sed -z 's/\n/:/g;s/:$/\n/')
export PATH
