if [ -n "${CUBICLE_NO_SET_BASH_PROMPT:-}" ]; then
  return 0
fi

parse_git_branch() {
  ref=$(git symbolic-ref HEAD -q 2>/dev/null)
  st=$?
  if [ $st -eq 1 ]; then
    echo "~~detached~~"
  elif [ $st -eq 0 ]; then
    echo "${ref#refs/heads/}"
  fi
}

nonprintable() {
  # this is necessary for control characters that don't ouptut text to avoid
  # problems with text wrapping
  echo "\[$1\]"
}

color() {
  # $1 is color intensity (0 for normal or 1 for bright/bold)
  # $2 should be 30 for black, 31 for red, 32 for green, 33 for yellow,
  #              34 for blue, 35 for magenta, 36 for cyan, or 37 for white
  # $3 is the text to color
  echo "$(nonprintable "\e[$1;$2m")$3$(nonprintable "\e[m")"
}

prompt_command() {
  status=$?
  git=$(parse_git_branch)
  title=''
  prompt=''

  # Print warning when previous command fails.
  if [ $status -ne 0 ]; then
    prompt=$(color 1 31 "Command exited with status $status")'\n'
  fi

  # Cubicle environment name (or username and host)
  prompt=$prompt$(color 1 33 "${CUBICLE:-'\u@\h'}")$(color 1 37 ':')
  title=$title${CUBICLE:-'\u@\h'}:

  # Working directory
  prompt=$prompt$(color 1 32 '\w')
  title=$title'\w'

  # Git branch
  if [ -n "$git" ]; then
    prompt=$prompt$(color 1 37 ':')$(color 1 33 "$git")
    title=$title":$git"
  fi

  # End
  prompt=$prompt$(color 1 37 '\$ ')

  xterm_title=$(nonprintable "\e]2;$title\a")
  PS1="$xterm_title$prompt"
}

PROMPT_COMMAND='prompt_command'
