# Deploying to a VPS

The app runs as one Docker container. TLS and the login come from Caddy. Two layouts:

- **Dedicated server:** `docker-compose.prod.yml` runs the app and Caddy together (`make prod-docker`).
- **Server that already runs Caddy** (ports 80/443 taken): run only the app, published on `127.0.0.1:8000` through `docker-compose.vps.yml`, and add a site block to the host's Caddy. This is what `make deploy` does.

## Deploy an update

```bash
make deploy        # reads DEPLOY_HOST and DEPLOY_PATH from .env
```

`scripts/deploy.sh` checks ssh, copies the working tree with rsync (never `.env`, `data/`, `.git`, `target/` or `node_modules/`), builds the image on the server, restarts the app and waits until `/api/chat/status` answers. It deploys what is on disk, not a commit, so commit first if you want the server and the branch to match. The first build takes about five minutes on 2 vCPUs (Rust compile); later ones reuse cached layers unless `Cargo.toml` changed.

## First-time setup (server with its own Caddy)

1. **DNS.** An `A` record for the host name pointing at the server IP. On Cloudflare use "DNS only", or Caddy cannot get its certificate.
2. **SSH key.** `ssh-copy-id user@host`, then set `DEPLOY_HOST=user@host` in your local `.env`.
3. **Docker and swap** (a 2 GB server needs swap for the Rust build):
   ```bash
   sudo fallocate -l 3G /swapfile && sudo chmod 600 /swapfile && sudo mkswap /swapfile && sudo swapon /swapfile
   echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab
   sudo apt-get install -y docker.io docker-compose-v2
   ```
4. **Server `.env`** in `~/vw/.env` (`chmod 600`). The compose file needs the three Caddy variables to be set even when the host's Caddy is used, so give them placeholders:
   ```
   SITE_ADDRESS=vw.example.com
   BASIC_AUTH_USER=admin
   BASIC_AUTH_HASH=unused-host-caddy
   APP_UID=1000        # id -u
   APP_GID=1000        # id -g
   AI_API_KEY=...
   AI_BASE_URL=https://ai.sumopod.com/v1
   AI_MODEL=gpt-4o-mini
   ```
   `~/vw/data` must be writable by `APP_UID:APP_GID`; it holds the SQLite file.
5. **Caddy site block** in the host's `/etc/caddy/Caddyfile`. Make the hash with `caddy hash-password --plaintext '...'`:
   ```
   vw.example.com {
       encode zstd gzip
       basic_auth {
           admin <hash>
       }
       # flush at once so the chat answer streams (Server-Sent Events)
       reverse_proxy 127.0.0.1:8000 {
           flush_interval -1
       }
   }
   ```
   Then `sudo caddy validate --config /etc/caddy/Caddyfile && sudo systemctl reload caddy`. Caddy gets the certificate by itself.
6. `make deploy`.

## Operations

- Logs: `ssh host 'sudo docker logs -f vw-app-1'`.
- Restart: `ssh host 'cd ~/vw && sudo docker compose -f docker-compose.prod.yml -f docker-compose.vps.yml restart app'`.
- Backup: the data is the SQLite file in `~/vw/data/` (the graph is rebuilt from `ontology/` and `rules/` at start). Copy it with cron or `sqlite3 erp.db '.backup ...'`. Not set up yet.
- Change the login: replace the hash in the Caddyfile block and reload.
- The chat key lives only in the server `.env`. It is never sent to the browser.
