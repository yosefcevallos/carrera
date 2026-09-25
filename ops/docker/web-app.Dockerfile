# syntax=docker/dockerfile:1.7
# Build context: repo root. Build: docker build -f ops/docker/web-app.Dockerfile --build-arg ... .
FROM node:24-alpine AS base
RUN corepack enable && corepack prepare pnpm@9.15.0 --activate
WORKDIR /app

FROM base AS deps
COPY web-app/package.json web-app/pnpm-lock.yaml ./
# --ignore-scripts: skips native builds (usb for Ledger, utf-8-validate) that the browser bundle never uses.
RUN --mount=type=cache,target=/root/.local/share/pnpm/store pnpm install --frozen-lockfile --ignore-scripts

FROM base AS build
# NEXT_PUBLIC_* are inlined at build time.
ARG NEXT_PUBLIC_DATA_SOURCE=rpc
ARG NEXT_PUBLIC_SUPABASE_URL
ARG NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY
ARG NEXT_PUBLIC_PROGRAM_ID=GH45ANLzg1t6rNnaoNqE39rN1rKGXFQXPZnNxvbhUmYw
ARG NEXT_PUBLIC_XSTOCK_MINTS
ARG NEXT_PUBLIC_USDC_MINT=EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
ARG NEXT_PUBLIC_RPC_URL=/api/rpc
ARG NEXT_PUBLIC_KEEPER_URL=/api/keeper
ARG NEXT_PUBLIC_OPS_ALLOWED_WALLETS
ENV NEXT_PUBLIC_DATA_SOURCE=$NEXT_PUBLIC_DATA_SOURCE \
    NEXT_PUBLIC_SUPABASE_URL=$NEXT_PUBLIC_SUPABASE_URL \
    NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY=$NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY \
    NEXT_PUBLIC_PROGRAM_ID=$NEXT_PUBLIC_PROGRAM_ID \
    NEXT_PUBLIC_XSTOCK_MINTS=$NEXT_PUBLIC_XSTOCK_MINTS \
    NEXT_PUBLIC_USDC_MINT=$NEXT_PUBLIC_USDC_MINT \
    NEXT_PUBLIC_RPC_URL=$NEXT_PUBLIC_RPC_URL \
    NEXT_PUBLIC_KEEPER_URL=$NEXT_PUBLIC_KEEPER_URL \
    NEXT_PUBLIC_OPS_ALLOWED_WALLETS=$NEXT_PUBLIC_OPS_ALLOWED_WALLETS \
    NEXT_TELEMETRY_DISABLED=1
COPY --from=deps /app/node_modules ./node_modules
COPY web-app/ ./
RUN pnpm build

FROM node:24-alpine AS runtime
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 PORT=3000 HOSTNAME=0.0.0.0
WORKDIR /app
RUN addgroup -S app && adduser -S app -G app
COPY --from=build --chown=app:app /app/.next/standalone ./
COPY --from=build --chown=app:app /app/.next/static ./.next/static
COPY --from=build --chown=app:app /app/public ./public
USER app
EXPOSE 3000
# Runtime env: SERVER_RPC_URL (upstream Solana RPC, key stays here), KEEPER_URL (http://keeper:8787).
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s CMD wget -qO- http://127.0.0.1:3000/ >/dev/null || exit 1
CMD ["node", "server.js"]
