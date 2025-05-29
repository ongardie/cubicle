#!/usr/bin/env nu

use std

def vcs_describe [] {
    loop {
        if ('.jj' | path exists) {
            return (jj_describe)
        }
        if ('.git' | path exists) {
            return (git_describe)
        }
        if (pwd) == '/' {
            return ""
        }
        cd ..
    }
}

def jj_describe [] {
    try {
        (
            jj
            --ignore-working-copy
            --quiet
            show
            --no-patch
            --template 'change_id.shortest()'
            err> (std null-device)
        )
    } catch {
        ""
    }
}

def git_describe [] {
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

    # Version control
    let vcs = vcs_describe
    if $vcs != "" {
        $visible += $":(ansi yellow_bold)($vcs)(ansi reset)"
        $title += $":($vcs)"
    }

    $"(ansi title)($title)(ansi reset)($visible)"
}

def prompt_command_right [] {
    let ok = $env.LAST_EXIT_CODE == 0
    let color = if $ok {
        "green"
    } else {
        "red"
    }
    let ms = $env.CMD_DURATION_MS | into int
    if $ms >= 1000 {
        $"(ansi $color)($ms / 1000 | math round -p 1) s(ansi reset)"
    } else if $ms >= 10 or not $ok {
        $"(ansi $color)($ms) ms(ansi reset)"
    } else {
        ""
    }
}

$env.PROMPT_COMMAND = { prompt_command }
$env.PROMPT_COMMAND_RIGHT = { prompt_command_right }
$env.PROMPT_INDICATOR = $"(ansi white_bold)$(ansi reset) "
$env.PROMPT_INDICATOR_VI_INSERT = $env.PROMPT_INDICATOR
$env.PROMPT_INDICATOR_VI_NORMAL = $"(ansi red_bold)@(ansi reset) "
