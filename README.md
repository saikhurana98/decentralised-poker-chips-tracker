---
type: Note
---

# Poker Bank

Live at [poker.makesomething.in](https://poker.makesomething.in). A shared cash-game chip tracker for 2–10 people playing with physical cards. No accounts, virtual cards, payment processing, or hand evaluation. The original offline app is preserved in `Poker Bank.html`; its `pokerbank-v1` browser saves are unchanged and are not imported into multiplayer sessions.

## Play

The home screen lists all sessions that have not ended, with table names, room IDs, player counts, and Join buttons. The list updates over WebSockets; table names and keys are public. Ending a session removes it from the list.

1. Create a table with your name, table name, currency, value per chip, and default buy-in. Defaults are INR, ₹1/chip, 1,000 chips, and blinds 5/10.
2. Open **Invite friends** to share the eight-character key, link, or locally generated QR. Each player joins from their phone. The host approves a seat and records the chips after confirming the real-world buy-in. The host must approve their own seat to play; they can also stay off-table and host.
3. Deal real cards and start a hand. Blinds post automatically. Each player acts on their own phone: fold, check, call, raise **to** a total, or all-in. After each round, the host confirms the next physical community cards are dealt. All-ins skip further betting once no competing stacks can act; finish dealing real cards before selecting winners.
4. At showdown, the host selects every winner for **each pot**, including tied winners. Folded players are ineligible. Odd chips go clockwise from the dealer. Uncalled amounts are returned automatically.
5. Between hands, approve new players, rebuy, cash out, swap seats, change blinds, or start the next hand. The dealer moves clockwise among funded seats. A late join waits for the next hand. There is no automatic timeout, fold, or reconnection takeover. The host can explicitly control a turn with the player's agreement.
6. Host controls include pause/resume, transfer host, and **Undo game action**. Undo is visible in activity and restores gameplay only, up to 50 actions. Buy-in/cashout entries are permanent; recording cash or changing seats/blinds clears gameplay undo history. Wrong cash entries are corrected by recording an opposite transaction, with agreement at the table.
7. **End & cash out all** closes the session and records remaining stacks as cashouts. The ledger shows chips and money, final net positions, suggested transfers, and a JSON export. Confirm and make payments outside the app. An ended table cannot resume.

Keep browser site data on the original device: a private random credential in `pokerbank-seats-v2` localStorage reconnects the same seat after reload or server restart. Invite links contain only the table key and never give control of another seat. Switching tables preserves saved seats. Transfer host **before** clearing browser data or changing devices; there is no password or account recovery. Anyone holding a table key may request a seat and see the table after joining; hosts approve participation. Do not use a table key as a secret financial document link.

Amounts use whole chips. A table may hold at most 10,000,000 chips; each transaction is 1–10,000,000. Chip value uses integer currency minor units (₹0.01–₹1,000 per chip; INR/USD/EUR/GBP). Monetary calculations stay within safe integer bounds. This is a no-limit, four-street betting tracker, with cash-game dealer rotation; antes, straddles, missed-blind debt, tournaments, and multiple boards are not supported. A host must determine physical-card winners.

## Local development

Node 24 LTS is recommended (minimum 22.13 with `node:sqlite`). Plain HTML/CSS/browser JavaScript; no frontend build.

```sh
npm ci
npm test
npm start
```

Open [localhost:8080](http://localhost:8080). Default storage is `data/poker.db`. `PUBLIC_ORIGIN` must exactly match the browser origin; override it and `PORT` for a different address. The previous offline file can still be opened directly. Current components:

- `engine.js`: pure authoritative betting and ledger transitions.
- `server.js`: HTTP assets and join/create APIs, authenticated WebSockets, SQLite persistence.
- `public/`: responsive green table and host/player controls.
- `test/`: Node test runner engine and real WebSocket integration tests.
- `deploy/`: consistent database backup, systemd units, and public Traefik route.

`npm test` passes 25 tests, including 100 simulated multiway hands, heads-up order, short blinds, minimum raises and cumulative short all-ins, side pots, uncalled refunds, ties/odd chips, undo/cash invariants, host authorization, cross-origin/cross-table rejection, duplicate/stale commands, and restart recovery. Browser QA completed on Chromium at 1440×1100 desktop and 390×844 phone through both localhost and the public HTTPS/WSS origin: create/join/approval, calls/checks/folds, three community-card confirmations, payouts/next hand, reload, QR, ten seats, and escaped malicious player names. No runtime/CSP errors or horizontal overflow. Phone betting controls stay fixed inside the viewport. A live container restart during an all-in showdown recovered the same seat, hand, revision and ledger; payout, end-session cashouts and monetary net settlement then completed. Disposable verification tables were removed. Screenshots are in `attachments/`.

## Persistence and limits

All accepted commands include an ID and expected revision. The server validates rules and authority, commits the new snapshot and deduplication record in one SQLite transaction with WAL and FULL synchronization, then acknowledges/broadcasts. Duplicate IDs return the original accepted revision without executing again. Stale actions are rejected and refreshed; the player reviews and retries. A command awaiting acknowledgement is resent with the same ID after reconnecting. No action succeeds locally while disconnected.

Credentials are 256-bit random tokens; only SHA-256 hashes are stored server-side. Exact-origin checks cover WebSockets and writes. Payload, connection, per-IP and per-player rate limits bound abuse; limits are per process and reset on restart. Trusted forwarded IPs are accepted only from the configured ingress address. Static CSP disallows inline scripts and styles. Player/table names are escaped before HTML insertion. Requests are limited to 8 creations/hour and 25 joins/hour/IP, 30 create/join requests/minute/IP; 500 simultaneous sockets, 10,000 retained tables, 20 pending requests and 40 total participant identities/table. Tables with no connections or activity for 90 days are removed hourly with credentials and deduplication records. A session still open on a connected phone is retained. There is no end-to-end encryption; the home server stores the table ledger.

## Home-server deployment

Production topology (verified 8 November 2026):

```text
poker.makesomething.in → wildcard DNS CNAME nb.makesomething.in
→ public 103.59.195.210:443 → CT117 Traefik (192.168.42.38)
→ CT116 poker-bank container (192.168.42.20:8087 → 8080)
→ Docker volume poker-bank-data (/data/poker.db)
```

The public route is **CT117**, not CT103/CT119 Caddy. It uses Traefik's existing `letsencrypt` TLS challenge resolver. The isolated `poker-bank` router/service was merged into `/root/traefik-dynamic.yaml`, retaining the existing TCP configuration. Original: `/root/traefik-dynamic.yaml.poker-20261108-193113`. No DNS change was needed. An initial CT103 route was removed by restoring `/etc/caddy/Caddyfile.poker-20261108-190224`; CT103 is back to its original config.

Source deployment is `/opt/poker-bank` on CT116. `compose.yaml` runs as the non-root `node` user, read-only root filesystem, dropped capabilities, 256 MB RAM/one CPU limit, bounded logs, persistent volume and health check, with `unless-stopped` restart. `TRUSTED_PROXIES=192.168.42.38`. There is no automatic GitHub deployment. Do not commit from the unrelated parent Notes repository.

After copying the project files to CT116 `/opt/poker-bank`:

```sh
cd /opt/poker-bank
docker compose up -d --build
docker inspect --format '{{.State.Health.Status}}' poker-bank
docker logs --tail 30 poker-bank
curl -fsS http://192.168.42.20:8087/healthz
```

Strict TLS was verified using Node fetch and Chromium; the workstation's old macOS curl trust bundle did not recognize the newly issued Let's Encrypt YR1 chain. Do not disable TLS verification for regular use.

A daily `poker-bank-backup.timer` runs at 04:20 host time (up to ten minutes randomized delay), including catch-up after downtime. It invokes Node SQLite's backup API inside the running container, retaining the newest 14 consistent backups in `/data/backups/`. A manual backup was tested successfully.

```sh
systemctl status poker-bank-backup.timer
systemctl start poker-bank-backup.service
journalctl -u poker-bank-backup.service -n 15
docker exec poker-bank ls -lh /data/backups
```

Backups are on the same Docker volume: copy them to separate storage for disk/host-loss protection. Never copy only a live database file while WAL is active.

To restore, first create a current backup, stop `poker-bank`, and use a temporary container mounting `poker-bank-data` to replace `/data/poker.db` with the selected backup, remove obsolete `/data/poker.db-wal` and `/data/poker.db-shm`, and retain UID/GID 1000 ownership. Start the container and check health and a known table. Do not delete the volume during upgrades.

For an app rollback, rebuild/deploy the previous source using the same volume. For route rollback, restore CT117's recorded original dynamic file **in place** (single-file bind mount) and verify Traefik reload; this removes public access to Poker Bank while leaving other routes untouched. Do not overwrite the backup or publish proxy configs containing provider credentials.
