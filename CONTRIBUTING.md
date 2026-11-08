# Contributing

## Required workflow

Use a focused `feat/`, `fix/`, or `chore/` branch and open a pull request against `main`. Keep each change reviewable. Use imperative Conventional Commit subjects, for example `fix: return uncalled all-in bets`. Describe the user-visible behavior, relevant issue, validation, and rollback implications. Include desktop and phone screenshots for UI changes. The maintainer reviews all changes; do not merge a failing CI run.

## Architecture and type safety

Rust owns poker rules, authorization, persistence, WebSockets, QR generation, and backups. Browser code belongs in `web/` and uses strict TypeScript. Do not introduce a JavaScript backend, duplicate poker rules in the browser, or edit generated `public/app.js`. Keep stored state and wire contracts compatible with existing sessions. Validate untrusted inputs at both Rust and browser boundaries.

Use `cargo fmt`, Clippy with warnings denied, strict TypeScript, type-aware ESLint, and Prettier. No `any`, `@ts-ignore`, unchecked non-null assertions, lint suppression, or unsafe Rust without an explicit reviewed justification. Avoid production panics and unchecked numeric conversions. Do not weaken compiler flags, lint rules, or CI to make a change pass. Commit both lockfiles when dependencies change.

## Local checks

Install stable Rust with rustfmt/Clippy and Node 24, then run:

```sh
npm ci
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
npm audit --audit-level=moderate
```

Run `cargo run --locked` and open `http://localhost:8080`. `PUBLIC_ORIGIN` must match the browser origin exactly. Build the browser assets after TypeScript edits.

## Regression requirements

Poker changes require targeted tests for affected rules and chip conservation. Cover heads-up order, short blinds, minimum raises, all-in reopening, folded contributions, side pots, ties, and odd chips when applicable. Server changes must test authorization, cross-table isolation, stale revisions, duplicate commands, and restart recovery. Browser changes must work at 390px and desktop widths without runtime errors or horizontal overflow.

## Data and deployment

Never commit credentials, private keys, databases, browser seat tokens, or production player data. Use disposable local tables. Cash ledger entries are permanent; gameplay undo must not erase them. Main builds deploy automatically after CI passes: database and protocol changes must support rollback, or explicitly require a reviewed maintenance migration. Back up the database before applying storage changes. Do not modify shared proxy routes or unrelated homeserver services.
