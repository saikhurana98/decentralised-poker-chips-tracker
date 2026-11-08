# Repository Guidelines

## Project Structure & Module Organization

`Poker Bank.html` is the entire application: HTML markup, inline CSS, and JavaScript. It tracks poker chips, blinds, bets, pots, and payouts while players use physical cards. There are currently no separate source, test, or asset directories. Keep small changes in this file; if the app grows, split code only with a clear reason and update the local run instructions.

## Build, Test, and Development Commands

No package manager, build step, or development server is configured. Open `Poker Bank.html` directly in a browser to run the app. For a simple local server, run `python3 -m http.server 8000` from this directory and visit `http://localhost:8000/Poker%20Bank.html`. Reload the page after edits. Browser developer tools are the primary way to inspect runtime errors and stored state.

## Coding Style & Naming Conventions

Use plain HTML, CSS, and browser JavaScript; avoid adding a framework or dependency for a small change. Follow the surrounding compact style when editing existing functions. Give new functions descriptive names, use `const` for values that do not change, and keep game state transitions separate from rendering where practical. Preserve the current single-file structure and CSS custom properties in `:root` for theme colors. The app currently has no formatter or linter configuration.

## Testing Guidelines

There is no automated test framework or coverage target. Test changed flows manually in a browser, including a new game, resume after reload, undo, blinds, folds, raises, all-ins, side pots, split pots, and the next hand when relevant. Check both a narrow mobile viewport and a desktop viewport. If adding automated tests, document the command and name tests after the behavior they verify.

## Commit & Pull Request Guidelines

This directory has no project-specific commit history to establish a message convention. Use short, imperative commit subjects such as `Fix side-pot payout`. Pull requests should describe the user-visible change, list the browser flows checked, and include screenshots for layout changes. Link an issue when one exists.

## Data & Security

Game state is saved in browser `localStorage` under `pokerbank-v1`. Keep saved-game compatibility in mind when changing its shape. Treat player names as untrusted input before inserting them into HTML.
