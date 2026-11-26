# Tidy

A fast, native Mac cleaner that lives in your menu bar, shows where your space actually goes, and warns you
before your disk fills up. Rust core (Tauri v2) · React + shadcn/ui interface · macOS first.

## Features
- **Smart Clean**: app caches, logs, Trash, Xcode build data and archives, simulator caches, package-manager
  caches (npm, Yarn, pnpm, Gradle, Cargo), `~/.cache` tool caches, local AI models (Hugging Face, Ollama),
  stale `node_modules` / Rust `target/` / Python virtualenvs, old installers, iOS backups and huge files.
  Safe items are pre-selected; anything slow to regenerate or user-authored (including emptying the Trash)
  never is.
- **Where your space goes**: sizes the usual hidden hogs (Docker disk image, simulators, Xcode, AI models,
  Photos, Messages, Mail, iCloud Drive copies, Homebrew) with a note on how to reclaim each, plus an
  explanation of purgeable space and Time Machine local snapshots.
- **Usage over time**: disk usage recorded every 15 minutes, charted with a "full in ~N days" forecast.
- **Storage Explorer**: drill into any readable folder (or the whole disk) with sizes, file-type breakdown,
  biggest files, and what grew since you last looked. Reveal in Finder or move to Trash.
- **Tools**: delete Time Machine local snapshots, remove unavailable iOS simulators, run `brew cleanup`,
  always via the official command, shown to you before it runs.
- **History**: an audit log of every item Tidy removed, trashed or failed to remove, and every tool run.
- **Menu bar**: popover with free space, status, forecast, top space users, preview-then-confirm Quick
  Clean, alert toggle and snooze. The icon shows a ring when storage is getting full and a dot when it's
  critical. Optional menu-bar-only mode (no Dock icon).
- **Storage alerts**: warning/critical thresholds with hysteresis, a daily re-notify cap, and snooze.
- **Safety first**: scans are read-only; every deletion passes a guard (inside `$HOME`, never protected
  folders, credentials, iCloud Drive, Photos/Music libraries or git repositories; case-insensitive; symlinks
  unlinked, not followed). Moves to Trash never overwrite. Tidy never runs sudo; admin actions use the
  macOS password dialog.
- **No surprise privacy prompts**: Tidy reads nothing in Desktop, Documents, Downloads, Photos or Music at
  launch or in the background. Those are only read when you start a Smart Scan, open a folder in the
  Explorer, or press Measure, and the app tells you beforehand that macOS may ask.
- **Full Disk Access aware**: unreadable locations are flagged, never silently counted as 0 B.

See [DESIGN.md](DESIGN.md) for the architecture.

## Install
1. Download `Tidy_<version>_universal.dmg` from the [latest release](https://github.com/manesid/tidy-mac/releases/latest)
   (runs on Apple Silicon and Intel, macOS 12 or later).
2. Open the dmg and drag **Tidy** into **Applications**.
3. First launch: Tidy is open source and not signed with a paid Apple Developer ID, so macOS blocks it the
   first time. Allow it once:
   - **macOS 15 and later:** open Tidy, click **Done** on the warning, then go to System Settings →
     Privacy & Security, scroll down and click **Open Anyway** next to Tidy.
   - **macOS 12 to 14:** right-click Tidy in Applications, choose **Open**, then **Open** again.
   - **Or, in Terminal:** `xattr -dr com.apple.quarantine /Applications/Tidy.app`

   After that it opens normally. If you'd rather not trust a downloaded build, build it from source (below);
   apps you build yourself are not blocked.

The SHA-256 of each dmg is listed in its release notes.

## Develop
Requirements: Rust (stable), Node 20+.

```sh
npm install
npm run tauri dev          # run the app
cargo test --manifest-path src-tauri/Cargo.toml   # engine tests
npm run tauri build -- --bundles app,dmg          # build Tidy.app / .dmg
```

For complete results grant **Full Disk Access** to Tidy in System Settings → Privacy & Security. Without it,
Mail downloads, iOS backups, Messages, Mail and Safari data are skipped and shown as "needs access"; the
rest works normally.
History is stored locally in `~/Library/Application Support/app.tidy.mac/tidy.db`; nothing leaves your Mac.

## Roadmap
Duplicate finder · app uninstaller · scheduled auto-clean · Windows & Linux.

## License
MIT
