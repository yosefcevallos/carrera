#!/usr/bin/env bash
# Deploy Carrera to the Hostinger VPS with docker compose. See ops/README.md for the cutover order.
#   ops/deploy-vps.sh --web-only                       # web container only; keeper/indexer stay on the laptop
#   ops/deploy-vps.sh --i-stopped-the-laptop-keeper    # full stack (web + keeper + indexer)
set -euo pipefail

VPS="${VPS:-root@srv1460793.hstgr.cloud}"
REMOTE_DIR="${REMOTE_DIR:-/opt/carrera}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
CFG="$HOME/.config/carrera"
ENV_FILE="$REPO/ops/docker/.env"
MODE=""
for a in "$@"; do
  case "$a" in
    --web-only) MODE=web ;;
    --i-stopped-the-laptop-keeper) MODE=full ;;
    *) echo "unknown flag $a"; exit 2 ;;
  esac
done

cat <<'WARN'
================================================================================
 STOP the laptop keeper first:   launchctl bootout gui/$(id -u)/com.carrera.keeper
 Two keepers must never run against the same registry (the file lease is per host).
 Pass --i-stopped-the-laptop-keeper for a full deploy, or --web-only to leave the
 keeper and indexer on the laptop.
================================================================================
WARN
[ -n "$MODE" ] || { echo "refusing: choose --web-only or --i-stopped-the-laptop-keeper"; exit 1; }

# ---- compose env (build args + runtime) ---------------------------------------------------
if [ ! -f "$ENV_FILE" ]; then
  echo "creating $ENV_FILE from .env.example and local secrets"
  cp "$REPO/ops/docker/.env.example" "$ENV_FILE"
  HELIUS="$(grep '^HELIUS_RPC_URL=' "$REPO/.env.supabase" | cut -d= -f2-)"
  PUB="$(grep '^SUPABASE_PUBLISHABLE_KEY=' "$REPO/.env.supabase" | cut -d= -f2-)"
  SUPA="$(grep '^SUPABASE_URL=' "$REPO/.env.supabase" | cut -d= -f2-)"
  sed -i '' -e "s#^SERVER_RPC_URL=.*#SERVER_RPC_URL=$HELIUS#" \
            -e "s#^NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY=.*#NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY=$PUB#" \
            -e "s#^NEXT_PUBLIC_SUPABASE_URL=.*#NEXT_PUBLIC_SUPABASE_URL=$SUPA#" "$ENV_FILE"
  chmod 600 "$ENV_FILE"
fi
# shellcheck disable=SC1090
APP_HOST="$(grep '^APP_HOST=' "$ENV_FILE" | cut -d= -f2-)"
echo "app host: $APP_HOST   mode: $MODE   target: $VPS:$REMOTE_DIR"

# ---- ship the repo ----------------------------------------------------------------------
ssh "$VPS" "mkdir -p $REMOTE_DIR /etc/carrera && chmod 700 /etc/carrera"
rsync -az --delete \
  --exclude '.git' --exclude '.claude' --exclude 'node_modules' --exclude '.next' --exclude 'target' \
  --exclude 'dist' --exclude 'program/target' --exclude '.env' --exclude '.env.*' --exclude '.env.local' \
  "$REPO/" "$VPS:$REMOTE_DIR/"
scp -q "$ENV_FILE" "$VPS:$REMOTE_DIR/ops/docker/.env"

# ---- secrets for keeper + indexer (full mode) -------------------------------------------
if [ "$MODE" = full ]; then
  TMP="$(mktemp -d)"
  # Rewrite host paths for the container; status server must listen on all interfaces inside the container.
  sed -e 's#^keypair_path = .*#keypair_path = "/etc/carrera/keeper.json"#' \
      -e 's#^lease_path = .*#lease_path = "/var/lib/carrera/keeper.lease"#' \
      -e 's#^history_path = .*#history_path = "/var/lib/carrera/history.jsonl"#' \
      -e 's#^status_bind = .*#status_bind = "0.0.0.0:8787"#' \
      "$CFG/keeper.toml" > "$TMP/keeper.toml"
  grep -q '^status_bind = "0.0.0.0:8787"' "$TMP/keeper.toml" || echo 'status_bind = "0.0.0.0:8787"' >> "$TMP/keeper.toml"
  # Indexer reaches the keeper by compose service name.
  sed -e 's#^KEEPER_URL=.*#KEEPER_URL=http://keeper:8787#' "$CFG/indexer.env" > "$TMP/indexer.env"
  grep -q '^KEEPER_URL=' "$TMP/indexer.env" || echo 'KEEPER_URL=http://keeper:8787' >> "$TMP/indexer.env"
  # The container's working dir is read-only for the app user; the cursor lives on the state volume.
  sed -i '' -e '/^CURSOR_PATH=/d' "$TMP/indexer.env" && echo 'CURSOR_PATH=/var/lib/carrera/indexer-cursor.json' >> "$TMP/indexer.env"
  scp -q "$TMP/keeper.toml" "$TMP/indexer.env" "$CFG/keeper.json" "$VPS:/etc/carrera/"
  ssh "$VPS" "chmod 600 /etc/carrera/*"
  rm -rf "$TMP"
fi

# ---- build and run ----------------------------------------------------------------------
SERVICES="web"; [ "$MODE" = full ] && SERVICES="web keeper indexer"
ssh "$VPS" "cd $REMOTE_DIR && docker compose -f ops/docker/compose.yml --env-file ops/docker/.env build $SERVICES \
  && docker compose -f ops/docker/compose.yml --env-file ops/docker/.env up -d $SERVICES \
  && docker compose -f ops/docker/compose.yml --env-file ops/docker/.env ps"

# ---- verify -------------------------------------------------------------------------------
echo "waiting for https://$APP_HOST/ ..."
for i in $(seq 1 30); do
  code="$(curl -s -o /dev/null -w '%{http_code}' "https://$APP_HOST/" || true)"
  [ "$code" = 200 ] && { echo "web: 200"; break; }
  sleep 5
done
[ "$code" = 200 ] || echo "web not reachable yet over TLS (Traefik cert may still be issuing); try: curl -k https://$APP_HOST/"
if [ "$MODE" = full ]; then
  echo "keeper healthz (may take ~2 min for the first pass):"
  ssh "$VPS" "cd $REMOTE_DIR && docker compose -f ops/docker/compose.yml --env-file ops/docker/.env exec -T keeper curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8787/healthz" || true
  ssh "$VPS" "cd $REMOTE_DIR && docker compose -f ops/docker/compose.yml --env-file ops/docker/.env logs --tail 5 indexer" || true
fi
echo "done. logs: ssh $VPS 'cd $REMOTE_DIR && docker compose -f ops/docker/compose.yml logs -f'"
