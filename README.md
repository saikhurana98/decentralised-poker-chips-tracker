# Poker Bank

[![CI](https://github.com/saikhurana98/decentralised-poker-chips-tracker/actions/workflows/ci.yml/badge.svg)](https://github.com/saikhurana98/decentralised-poker-chips-tracker/actions/workflows/ci.yml)

A shared poker-chip tracker for 2–10 people playing with physical cards. Everyone acts from their own phone; the host manages seats, real-world buy-ins, community-card progression, and payouts.

**Play at [poker.makesomething.in](https://poker.makesomething.in).**

## Playing

1. Create a named table, choose currency, value per chip, and default buy-in. Defaults: INR, ₹1/chip, 1,000 chips, blinds 5/10.
2. Share the eight-character room ID, invite link, or QR. Active games appear in the public lobby with names and room IDs. The host approves each seat and buy-in, including their own seat if playing.
3. Deal physical cards. Players fold, check, call, raise to a total, or go all-in on their phones. After betting, the host confirms the next community cards.
4. At showdown, the host chooses winners for each main/side pot, including ties. The app refunds uncalled bets and divides odd chips clockwise from the dealer.
5. Between hands, approve late joins, rebuys, cashouts, seat changes, or blinds. Host controls include pause, explicit turn takeover, host transfer, and visible gameplay undo.
6. End the session to cash out remaining stacks and see net balances and suggested payments. Export the ledger as JSON. Payments happen outside the app.

Keep browser site data to reconnect to your seat. The browser holds a private device credential; sharing a room ID does not give control of another seat. Transfer hosting before clearing the host device's browser data. There is no account recovery. Table names and room IDs are public, and anyone can request a seat.

## Stack

- **Rust:** authoritative poker rules, typed commands/state, HTTP/WebSockets, SQLite transactions, QR generation, and consistent backups.
- **TypeScript:** browser interface and integration tests, with strict compiler options, type-aware ESLint, and Zod validation at message boundaries.
- **SQLite:** persistent tables, hashed seat credentials, permanent cash ledger, accepted command IDs, and revisions. The migration preserves the original SQLite schema and camelCase JSON protocol.
- **Docker:** non-root, read-only application container with a persistent `/data` volume, health checks, restart policy, and bounded resource/log usage.

`src/` contains Rust code. `web/` contains browser TypeScript and wire schemas. `public/` contains HTML/CSS and generated `app.js`. `test/` contains TypeScript WebSocket integration tests. `deploy/` contains release, backup, and systemd files. `Poker Bank.html` is the preserved offline predecessor; its browser saves are not imported into multiplayer tables.

## Development

Install stable Rust with rustfmt/Clippy and Node 24:

```sh
npm ci
npm run build
cargo run --locked
```

Open `http://localhost:8080`. Rebuild browser assets after TypeScript changes. Configure `PUBLIC_ORIGIN` to exactly match the browser address. SQLite defaults to `data/poker.db`; override `DB_PATH` for disposable testing. Run the gates in [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

```sh
npm run format:check
npm run lint
npm run build
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
npm run test:integration
npx playwright install chromium
npm run test:browser
```

The frontend enables `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, and unused-code checks. ESLint uses `strictTypeChecked` with zero warnings allowed. Prettier formats frontend/configuration files; rustfmt formats Rust. The minimatch override tracks the patched version required to avoid a transitive lint-tool vulnerability.

Validation includes 26 Rust regression tests, the multi-client TypeScript integration suite, and a Playwright phone gameplay test. Full desktop/phone QA also covers ten seats, QR invites, reconnects, side-pot payouts, and safe name rendering.

## CI/CD

Pull requests and main pushes run `.github/workflows/ci.yml`: formatting, ESLint, TypeScript/build, dependency audit, rustfmt, Clippy with warnings denied, Rust tests, actual multi-client WebSocket integration/restart tests, and a mobile browser gameplay smoke test. Third-party actions are pinned to commit SHAs. Pull requests have read-only workflow permissions and never publish releases.

After all checks pass on `main`, GitHub builds a static Linux amd64 Rust binary and browser assets, packages a release with SHA256 and its source commit, then publishes it to `deploy/linux-amd64`. Only the release job has repository write permission. The deployment branch retains the newest build rather than accumulating binaries.

The homeserver checks that branch every two minutes using a repository-specific **read-only deploy key**. It verifies the checksum, creates a database backup, builds a minimal runtime container, switches releases, and requires `/healthz` to report the exact source commit. A failed health check restores the previous image. GitHub Actions has no SSH access to the homeserver and stores no server credentials.

The database volume is preserved during upgrades. Current code changes must maintain saved-state and protocol compatibility. Automatic application rollback does not reverse database migrations; any incompatible schema change requires a reviewed migration and recovery plan.

## Hosting and operations

Copy source to a Docker host and configure the LAN bind address, `PUBLIC_ORIGIN`, and `TRUSTED_PROXIES` in `compose.yaml`. Terminate HTTPS at your reverse proxy, forwarding WebSocket upgrades to the application. The production route is managed separately from application releases; `deploy/traefik-poker.yaml` is the isolated route example.

```sh
docker compose up -d --build
docker logs --tail 30 poker-bank
docker exec poker-bank /app/poker-bank backup
systemctl status poker-bank-deploy.timer poker-bank-backup.timer
journalctl -u poker-bank-deploy.service -n 30
```

The backup CLI uses SQLite's online backup API and retains the latest 14 snapshots in `/data/backups` (`BACKUP_DIR` can override it). A daily systemd timer runs it. These copies share the data volume: copy snapshots to separate storage for host/disk-loss protection. Never copy just a live WAL database file.

To restore: take a current backup, stop the application, replace `/data/poker.db` with a selected consistent backup, remove stale `-wal` and `-shm` files, preserve UID/GID 1000 ownership, restart, and verify the recovered table. Never delete the Docker volume during an upgrade. For release rollback, redeploy a previous verified image with the same volume and matching `RELEASE_SHA`.

## Rules and limits

Whole chips only; at most 10,000,000 chips per table. Supported currencies: INR, USD, EUR, GBP, with integer minor-unit chip values. No-limit cash games, physical cards, four betting streets. No tournaments, antes, straddles, missed-blind debt, multiple boards, or automated card evaluation. Disconnections wait for reconnection or explicit host control; no automatic folds.

Every game command has a unique ID and expected revision. The server validates authorization/rules and commits state plus deduplication atomically before acknowledging/broadcasting. Reconnects retrieve current state and retry unacknowledged commands with the same ID. Cash entries are permanent; undo affects gameplay only. Buy-ins, cashouts, seating, and blind changes clear gameplay undo history.

Exact-origin checks, payload/connection/rate limits, hashed random credentials, CSP, and escaped names protect public access. Sessions without activity or connections expire after 90 days. There is no end-to-end encryption: the server stores the ledger. Never commit production databases, browser seat tokens, provider credentials, or private keys.
