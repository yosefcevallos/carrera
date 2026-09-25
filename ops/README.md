# ops

Two ways to keep Carrera running without this laptop.

## A. launchd on the Mac (`install-services.sh`)

Installs `com.carrera.keeper` and `com.carrera.indexer` as user agents that restart on crash and
on login. macOS denies launchd agents any access to `~/Desktop`, so the indexer runs from a copy
at `~/.config/carrera/indexer-runtime` and reads `~/.config/carrera/indexer.env`; re-run the script
after `pnpm build` in `indexer/`. Logs: `~/Library/Logs/carrera/`. Remove with `uninstall-services.sh`.

## B. Docker on the VPS (`deploy-vps.sh`, `docker/`)

Target: `root@srv1460793.hstgr.cloud` (Hostinger, Docker Manager, Traefik already running).
Three images built from the repo root: `web` (Next.js standalone, port 3000), `keeper`
(Rust, status server on 8787 inside the compose network only), `indexer` (Node). Secrets are
never in the images: the web container gets `SERVER_RPC_URL` at runtime and proxies the browser's
`/api/rpc` and `/api/keeper` calls; the keeper and indexer read `/etc/carrera/*` bind mounts.

### Modes

| Command | Runs on the VPS | Stays on the laptop |
|---|---|---|
| `ops/deploy-vps.sh --web-only` | web | keeper, indexer (the VPS `/ops` page will show the keeper as unreachable) |
| `ops/deploy-vps.sh --i-stopped-the-laptop-keeper` | web, keeper, indexer | nothing |

Only one keeper may run against the registry at a time: the leader lease is a file, so two hosts
would both believe they lead and double-send cranks. The program rejects duplicates, but the
second keeper burns SOL and muddles the ops history.

### Cutover order (full mode)

1. `launchctl bootout gui/$(id -u)/com.carrera.keeper` and `... com.carrera.indexer` on the laptop.
2. `ops/deploy-vps.sh --i-stopped-the-laptop-keeper`. First run creates `ops/docker/.env` from
   `.env.example` plus the local secrets; check `APP_HOST`, `TRAEFIK_NETWORK` and
   `TRAEFIK_CERTRESOLVER` before running again.
3. Wait for the keeper healthcheck (up to two minutes: lease wait plus first fast pass), then
   open `https://APP_HOST/ops`.
4. To move back: `docker compose ... stop keeper indexer` on the VPS, then `ops/install-services.sh`.

### Traefik assumptions and how to check them on the VPS

- Traefik listens on 80/443 with an entrypoint named `websecure` and a certificate resolver
  (default `letsencrypt`). Check: `docker inspect traefik --format '{{json .Args}}'` or
  `docker inspect traefik | grep -i -E 'entrypoints|certificatesresolvers'`.
- The network Traefik uses for discovery: `docker network ls` then
  `docker inspect traefik --format '{{json .NetworkSettings.Networks}}'`; put that name in
  `TRAEFIK_NETWORK`. Hostinger's Docker Manager usually names it after the Traefik project.
- Traefik must be started with the Docker provider (`--providers.docker`), otherwise labels are
  ignored. If any of this cannot be matched, use the standalone fallback:
  `docker compose -f ops/docker/compose.yml -f ops/docker/compose.standalone.yml --env-file ops/docker/.env up -d`
  which puts Caddy on 80/443 for `APP_HOST` with automatic TLS. Stop Traefik first or it will
  hold those ports.

### Keeper config inside the container

`deploy-vps.sh` rewrites `~/.config/carrera/keeper.toml` for the container: `status_bind =
"0.0.0.0:8787"`, `keypair_path = "/etc/carrera/keeper.json"`, lease and history under
`/var/lib/carrera` (a named volume). The indexer env gets `KEEPER_URL=http://keeper:8787`.

### Updating

Re-run the deploy script; it rsyncs the repo and rebuilds only changed layers. Web build args
(`NEXT_PUBLIC_*`) are baked at build time, so a change to them needs a rebuild, which the script
always does.
