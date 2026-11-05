# PokeGrid Remoto: página web + servidor Rust + Chromium headless (ver server/README.md).
# Build a partir da raiz do repositório: docker compose -f server/docker-compose.yml up -d --build

# 1) Página (remote.html + painel do Modo Eco)
FROM node:20-bookworm-slim AS web
WORKDIR /src
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY tsconfig.json vite.config.ts index.html tools.html remote.html ./
COPY src ./src
RUN npx tsc --noEmit && npx vite build

# 2) Servidor (reaproveita a contabilidade do app em src-tauri/src/domain)
FROM rust:1-slim-bookworm AS server
WORKDIR /src
COPY src-tauri/src/domain ./src-tauri/src/domain
COPY server/Cargo.toml server/Cargo.lock ./server/
COPY server/src ./server/src
RUN cd server && cargo build --release --locked

# 3) Imagem final: Debian + Google Chrome numa tela virtual.
# O chromium-headless-shell é classificado como cliente automático e o Cloudflare do jogo bloqueia.
# O Chrome de desktop, que é o que as outras VPS usam, abre numa tela Xvfb (PG_HEADED=1).
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates fonts-liberation fonts-noto-color-emoji tini wget xvfb \
 && wget -qO /tmp/chrome.deb https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb \
 && apt-get install -y --no-install-recommends /tmp/chrome.deb \
 && rm -rf /var/lib/apt/lists/* /tmp/chrome.deb \
 && ln -sf /usr/bin/google-chrome-stable /usr/local/bin/pg-chrome \
 && useradd --create-home --uid 1000 pokegrid \
 && mkdir -p /data \
 && chown pokegrid:pokegrid /data
COPY --from=server /src/server/target/release/pokegrid-server /usr/local/bin/pokegrid-server
COPY --from=web /src/dist /app/web
USER pokegrid
ENV PG_DATA=/data PG_WEB=/app/web PG_BIND=0.0.0.0:8080 PG_CHROME=/usr/local/bin/pg-chrome PG_HEADED=1
VOLUME /data
EXPOSE 8080
# tini como PID 1 recolhe os processos órfãos do Chromium (no Swarm/Dokploy não existe o init: true do compose)
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["pokegrid-server"]
