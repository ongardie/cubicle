# Allow comments with '#' on the console. Otherwise these command get run and
# end up showing silly things like:
#     zsh: command not found: #echo
#     Command exited with status 127
setopt INTERACTIVE_COMMENTS
