#!/bin/sh

export BINSTALL_DISABLE_TELEMETRY=true
# disable quick-install strategy by default (a less trusted third-party)
export BINSTALL_STRATEGIES=crate-meta-data,compile
