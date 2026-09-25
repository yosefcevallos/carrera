# syntax=docker/dockerfile:1.7
# Build context: repo root.
FROM rust:1.91-bookworm AS build
WORKDIR /src
COPY keeper/Cargo.toml keeper/Cargo.lock ./
# Warm the dependency cache with a stub main, then build the real source.
RUN mkdir -p src && echo 'fn main(){}' > src/main.rs \
 && cargo build --release --locked 2>/dev/null || true
COPY keeper/ ./
RUN touch src/main.rs && cargo build --release --locked

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
RUN useradd -r -m -d /var/lib/carrera carrera
COPY --from=build /src/target/release/carrera-keeper /usr/local/bin/carrera-keeper
# Mount: /etc/carrera/keeper.toml (must set status_bind = "0.0.0.0:8787", keypair_path = "/etc/carrera/keeper.json",
# lease_path = "/var/lib/carrera/keeper.lease", history_path = "/var/lib/carrera/history.jsonl"), /etc/carrera/keeper.json.
ENV CARRERA_KEEPER_CONFIG=/etc/carrera/keeper.toml
VOLUME ["/var/lib/carrera"]
USER carrera
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=5s --start-period=120s CMD curl -fsS http://127.0.0.1:8787/healthz >/dev/null || exit 1
CMD ["carrera-keeper", "--config", "/etc/carrera/keeper.toml", "run", "--feed", "live"]
