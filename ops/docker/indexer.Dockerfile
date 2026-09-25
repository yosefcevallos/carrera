# syntax=docker/dockerfile:1.7
# Build context: repo root.
FROM node:24-alpine AS base
RUN corepack enable && corepack prepare pnpm@9.15.0 --activate
WORKDIR /app

FROM base AS build
COPY indexer/package.json indexer/pnpm-lock.yaml ./
RUN --mount=type=cache,target=/root/.local/share/pnpm/store pnpm install --frozen-lockfile
COPY indexer/ ./
RUN pnpm build && pnpm prune --prod

FROM node:24-alpine AS runtime
ENV NODE_ENV=production
WORKDIR /app
RUN addgroup -S app && adduser -S app -G app
COPY --from=build --chown=app:app /app/dist ./dist
COPY --from=build --chown=app:app /app/node_modules ./node_modules
COPY --from=build --chown=app:app /app/package.json ./package.json
USER app
# Mount /etc/carrera/indexer.env (SUPABASE_URL, SUPABASE_SERVICE_ROLE_KEY, PROGRAM_ID, RPC_URL, KEEPER_URL=http://keeper:8787, SOURCE=poll, ...).
HEALTHCHECK --interval=60s --timeout=5s --start-period=30s CMD pgrep -f "dist/main.js" >/dev/null || exit 1
CMD ["node", "--env-file=/etc/carrera/indexer.env", "dist/main.js"]
