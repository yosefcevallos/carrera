#!/usr/bin/env bash
# Stop and remove the keeper and indexer launchd agents.
set -euo pipefail
for name in keeper indexer; do
  label="com.carrera.$name"
  launchctl bootout "gui/$(id -u)/$label" 2>/dev/null && echo "stopped $label" || echo "$label not loaded"
  rm -f "$HOME/Library/LaunchAgents/$label.plist"
done
