#!/usr/bin/env nu

use std

def parse_git_branch [] {
    let ref = git symbolic-ref HEAD -q | complete
    match $ref.exit_code {
        0 => $"($ref.stdout | parse "refs/heads/{ref}\n" | $in.0.ref)"
        1 => "~~detached~~"
        _ => ""
    }
}

def prompt_command [] {
    let st = $env.LAST_EXIT_CODE
    mut title = ""
    mut visible = ""

    # Print warning when previous command fails.
    if $st != 0 {
        $visible += $"(ansi red_bold)Command exited with status ($st)(ansi reset)\n"
    }

    # Cubicle environment name or username & host.
    if "CUBICLE" in $env {
        $visible += $"(ansi yellow_bold)($env.CUBICLE)(ansi reset):"
        $title += $"($env.CUBICLE):"
    } else if "SSH_CLIENT" in $env or "SUDO_USER" in $env {
        $visible += $"(ansi yellow_bold)($env.USER)@($env.HOST)(ansi reset):"
        $title += $"($env.USER)@($env.HOST):"
    }

    # Working directory
    let cwd = if (pwd | str starts-with $env.HOME) {
        pwd | str replace $env.HOME "~"
    } else {
        pwd
    }
    $visible += $"(ansi green_bold)($cwd)(ansi reset)"
    $title += $cwd

    # Git branch
    let git = parse_git_branch
    if $git != "" {
        $visible += $":(ansi yellow_bold)($git)(ansi reset)"
        $title += $":($git)"
    }

    $"(ansi title)($title)(ansi reset)($visible)"
}

$env.PROMPT_COMMAND = {|| prompt_command}
$env.PROMPT_COMMAND_RIGHT = ""
$env.PROMPT_INDICATOR = $"(ansi white_bold)$(ansi reset) "
$env.PROMPT_INDICATOR_VI_INSERT = $env.PROMPT_INDICATOR
$env.PROMPT_INDICATOR_VI_NORMAL = $"(ansi red_bold)@(ansi reset) "
