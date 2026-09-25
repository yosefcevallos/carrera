#!/usr/bin/env bash
# Install the keeper and indexer as launchd user agents (survive sleep and reboot).
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
NODE="$(command -v node)"
NODE_DIR="$(dirname "$NODE")"
AGENTS="$HOME/Library/LaunchAgents"
mkdir -p "$AGENTS" "$HOME/Library/Logs/carrera" "$HOME/.config/carrera"
# macOS TCC denies launchd agents access to ~/Desktop, so the indexer runs from a copy under
# ~/.config/carrera and reads its env from there. Re-run this script after `pnpm build` in indexer/.
RT="$HOME/.config/carrera/indexer-runtime"
mkdir -p "$RT"
rsync -a --delete "$REPO/indexer/dist" "$REPO/indexer/node_modules" "$REPO/indexer/package.json" "$RT/"
cp "$REPO/indexer/.env" "$HOME/.config/carrera/indexer.env" && chmod 600 "$HOME/.config/carrera/indexer.env"
for name in keeper indexer; do
  label="com.carrera.$name"
  dst="$AGENTS/$label.plist"
  sed -e "s#__REPO__#$REPO#g" -e "s#__HOME__#$HOME#g" -e "s#__NODE__#$NODE#g" -e "s#__NODE_DIR__#$NODE_DIR#g" \
    "$REPO/ops/launchd/$label.plist" > "$dst"
  plutil -lint "$dst" >/dev/null
  launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
  launchctl bootstrap "gui/$(id -u)" "$dst"
  echo "installed $label"
done
sleep 2
launchctl list | grep carrera || true
echo "logs: $HOME/Library/Logs/carrera/{keeper,indexer}.log"
