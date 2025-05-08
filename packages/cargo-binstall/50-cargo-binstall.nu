#!/usr/bin/env nu

$env.BINSTALL_DISABLE_TELEMETRY = "true"
# disable quick-install strategy by default (a less trusted third-party)
$env.BINSTALL_STRATEGIES = "crate-meta-data,compile"
