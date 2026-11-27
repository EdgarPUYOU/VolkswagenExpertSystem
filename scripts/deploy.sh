#!/usr/bin/env bash
# Deploy to the VPS: copy the working tree, build the image there, restart the app, check it answers.
# Needs key-based ssh to DEPLOY_HOST (user@host). Set it in .env or the environment. See docs/deploy.md.
set -euo pipefail

cd "$(dirname "$0")/.."

# read DEPLOY_* from .env without sourcing it (it holds secrets)
env_value() { [ -f .env ] && grep -E "^$1=" .env | head -1 | cut -d= -f2- || true; }
HOST="${DEPLOY_HOST:-$(env_value DEPLOY_HOST)}"
DIR="${DEPLOY_PATH:-$(env_value DEPLOY_PATH)}"
DIR="${DIR:-vw}"
[ -n "$HOST" ] || { echo "Set DEPLOY_HOST (user@host) in .env or the environment." >&2; exit 1; }

SSH=(ssh -o BatchMode=yes -o ConnectTimeout=10)
COMPOSE="sudo docker compose -f docker-compose.prod.yml -f docker-compose.vps.yml"

echo "==> checking ssh to $HOST"
"${SSH[@]}" "$HOST" true || { echo "ssh failed: run ssh-copy-id $HOST first." >&2; exit 1; }
"${SSH[@]}" "$HOST" "test -f $DIR/.env" || { echo "$DIR/.env is missing on the server: see docs/deploy.md (first-time setup)." >&2; exit 1; }

echo "==> copying the working tree"
# no --delete: the server keeps its own .env and data/. Secrets and build output stay local.
rsync -az -e "ssh -o BatchMode=yes" \
  --exclude .git --exclude target --exclude node_modules --exclude dist \
  --exclude .env --exclude data --exclude .playwright-mcp \
  ./ "$HOST:$DIR/"

echo "==> building the image on the server (a few minutes the first time)"
"${SSH[@]}" "$HOST" "cd $DIR && $COMPOSE build app"

echo "==> restarting the app"
"${SSH[@]}" "$HOST" "cd $DIR && $COMPOSE up -d --no-deps app"

echo "==> waiting for the app to answer"
"${SSH[@]}" "$HOST" 'for i in $(seq 1 30); do curl -sf localhost:8000/health >/dev/null && echo "up" && exit 0; sleep 2; done; echo "app did not answer" >&2; exit 1'

echo "==> removing build leftovers"
# Only this build's residue: dangling images and build cache older than a day.
# No `volume prune`: the host is shared (mail server, Mailpit) and that would delete their unused volumes too.
# The app keeps its data in the bind mount ./data, which is not a Docker volume.
"${SSH[@]}" "$HOST" "sudo docker image prune -f && sudo docker builder prune -f --filter until=24h"

echo "==> done"
