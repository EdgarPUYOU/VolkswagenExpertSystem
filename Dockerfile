# Multi-stage build: Node builds the web bundle, Rust embeds it into one binary,
# a distroless image runs it. Build context is the repo root.

FROM node:24-slim AS web
WORKDIR /w
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.99-bookworm AS build
WORKDIR /src
COPY app/ app/
COPY --from=web /w/dist web/dist
RUN cargo build --release --manifest-path app/Cargo.toml

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/app/target/release/vw-erp /usr/local/bin/vw-erp
COPY ontology/ /ontology/
COPY rules/ /rules/
ENV ONTOLOGY_DIR=/ontology \
    RULES_DIR=/rules \
    SQLITE_PATH=/data/erp.db \
    BIND=0.0.0.0:8000
EXPOSE 8000
ENTRYPOINT ["/usr/local/bin/vw-erp"]
CMD ["serve"]
