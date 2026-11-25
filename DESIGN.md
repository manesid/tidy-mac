# Tidy — System Design

Native macOS cleaner with storage alerts. Rust core, Tauri v2 shell, React + shadcn/ui front end.
macOS first; Windows/Linux later through a single `platform` seam.

## Goals / non-goals
- Goals: find and reclaim space fast and **safely**; explain where space goes (including what macOS calls
  "System Data"); alert before the disk fills; stay light (idle < 30 MB RSS, ~0% CPU).
- Non-goals (for now): anti-malware, RAM "boosters", app uninstaller, duplicate finder, non-Mac platforms.

## Architecture

```
┌────────────────────────────── Tauri process ──────────────────────────────┐
│  WebView "main"     Overview · Smart Clean · Explorer · Tools · History ·   │
│                     Alerts & Menu Bar                                       │
│  WebView "popover"  menu bar popover (same bundle, chosen by window label)  │
│           ▲ typed commands + events ▼                                       │
│  engine/ (pure Rust, no Tauri deps, unit-tested)                            │
│    catalog   declarative list of cleanable categories + artifact rules      │
│    scanner   parallel size/discovery engine (rayon + walkdir)               │
│    guard     the ONLY gate allowed to authorise a deletion                  │
│    cleaner   executes plan: delete | trash | empty-trash; per-item outcome  │
│    explorer  read-only folder sizes, file kinds, biggest files, deltas      │
│    space     sizes well-known space hogs ("where did my space go")          │
│    store     SQLite: usage time series, audit log, explorer snapshots, kv   │
│    trend     least-squares growth + "full in ~N days"                       │
│    alerts    threshold state machine (hysteresis, cooldown)                 │
│    menubar   tray title, snooze windows, state badge drawing                │
│    disk      volume stats (statvfs) · settings JSON, atomic write           │
│  platform/   macos.rs: paths, FDA probe, trash, space locations, official   │
│              CLIs (tmutil, simctl, brew) and their output parsers           │
│  lib.rs      shell: commands, tray, popover window, notifications,          │
│              scheduler thread                                               │
└─────────────────────────────────────────────────────────────────────────────┘
```

Layering rule: `engine` never imports Tauri; `lib.rs` adapts it to the OS UI. All filesystem, SQLite and
subprocess work runs off the UI thread (`spawn_blocking` or the scheduler thread).

## Key decisions
1. **Declarative catalog.** A category = `{id, name, safety, disposal, source}`. Sources: children of a dir
   (with skip prefixes), fixed paths, files by extension, large files, stale project artifacts, Trash, or a
   union (`Many`). Adding a target is data, not control flow.
2. **Scan results live in Rust.** The UI receives opaque item ids (`s<generation>/<category>:<n>`);
   `clean(ids)` resolves them against the last scan held in memory, and ids from an older scan never resolve
   (the generation is part of the id). The webview can never submit an arbitrary path to delete. The one
   exception is `explorer_trash`: move to Trash only, and only for a path the current Explorer listing showed
   (the backend remembers what it listed), then guarded like everything else.
3. **Guard before every destructive op.** Canonicalise the parent, don't follow the final component (symlinks
   are unlinked; a final `..` is refused) → must be under `$HOME` → depth ≥ 2 → not a protected root
   (`~/Documents`, `~/Library`, …) → no `~/Library/<X>` as a whole (Caches, Group Containers, Preferences…;
   the catalog only targets children inside them) → not inside a protected subtree at any depth (`.ssh`,
   `.gnupg`, `.aws`, `.kube`, `.docker`, Keychains, iCloud Drive `Mobile Documents`, `CloudStorage`, Mail,
   Messages, Accounts, Safari, Calendars, Contacts, Notes) → not inside an app-managed library package
   (Photos, Music, TV, iMovie, Final Cut, Logic) → not a git work-tree root. All name comparisons are
   case-insensitive (default APFS is case-insensitive, so `~/library/keychains` is the Keychains folder).
   Called immediately before each delete/rename, never earlier (keeps the TOCTOU window to one syscall).
   Trash moves use `renamex_np(RENAME_EXCL)`, so a name collision picks `name 2.ext` and never overwrites;
   an item on another volume fails with a message instead of being copied.
4. **Disposal by risk.** Regenerable data (caches, logs, DerivedData) → permanent delete, pre-selected.
   The Trash itself is **Review**: it is the user's undo buffer (and where Tidy's own "To Trash" items go),
   so emptying it is never pre-selected and never part of Quick Clean. Emptying reports failure if any item
   could not be removed (no Finder/AppleScript fallback).
   Slow to regenerate (AI models, stale `node_modules`/`target`/venvs) → delete, never pre-selected.
   User-authored (large files, installers, iOS backups, Xcode archives) → Trash, never pre-selected.
5. **Stale artifacts by evidence, not by name.** A `target/` is only Rust output if the parent has
   `Cargo.toml` and it contains cargo markers; a `venv` only if it has `pyvenv.cfg`; a project is stale only if
   its manifests, lockfiles and directory are all older than 60 days. One walk of `$HOME` serves all rules.
6. **Model caches are never "safe".** `~/.cache` is scanned per tool with `huggingface` excluded; Hugging
   Face and Ollama models are a separate Review category (multi-GB, slow to re-download).
7. **Official CLIs for system-owned space.** Time Machine local snapshots (`tmutil`), unavailable
   simulators (`xcrun simctl delete unavailable`) and Homebrew (`brew cleanup`) are fixed-id tools. The UI
   shows the exact command and asks for confirmation. No sudo: snapshot deletion goes through
   `osascript … with administrator privileges`, so macOS shows its own password dialog. Snapshot dates are
   validated (`YYYY-MM-DD-HHMMSS`) and must be in the list tmutil just returned. `simctl` is only invoked when a
   full Xcode is selected, which avoids triggering the Command Line Tools installer prompt.
8. **Local history in SQLite** (`tidy.db` in the app config dir, WAL, migrations via `PRAGMA user_version`).
   - usage samples: at most one per 15 min; after 30 days thinned to one per day; kept 2 years.
   - audit log: every deleted, trashed or failed item and every tool run; kept 12 months.
   - explorer snapshots: per folder, compared against one at least 1 h old, so a re-visit shows what grew;
     5 generations per folder, 90 days. Only complete (non-cancelled) listings are stored.
   - kv: last space breakdown, so the popover can show it instantly.
   If the DB can't be opened the app still works; history features show an "unavailable" state.
9. **Alerts = pure state machine** `(usage, state, now) → (Option<Alert>, state)` with 2% hysteresis and 24 h
   re-notify cooldown. Snooze (1 h or until 08:00 next day, local time) suppresses evaluation.
10. **Menu bar.** Left click opens a popover window (borderless, always on top, native popover material,
    hides on blur or Esc); right click opens the menu. The template icon gets a shape badge (ring = warn,
    dot = critical), because template images can't carry color and must adapt to light/dark menu bars.
    The title next to the icon is configurable (free space / % used / none). "Menu bar only" switches the
    activation policy to Accessory (no Dock icon).
11. **Quick Clean is preview-then-confirm.** `quick_preview` scans Safe categories into a separate id
    namespace (`q/…`) and returns the total. Nothing is removed until the user presses a button labelled with
    that size; `quick_clean_commit` cleans exactly the previewed ids, and only within 10 minutes.
12. **Permissions honest.** Probe Full Disk Access; categories and space hogs that can't be read are flagged
    (and sizes shown as "≥") instead of silently reporting 0 B. Categories marked `needs_fda` (Mail
    downloads, iOS backups) are not read at all without FDA.
13. **Privacy prompts are user-initiated (invariant).** Reading Desktop, Documents, Downloads, the Photos
    or Music/TV libraries, iCloud Drive or another app's container makes macOS show a consent prompt. So:
    - Nothing walks folders at launch, in the scheduler (statvfs + SQLite only) or in the popover (cached
      breakdown only). The Explorer opens on a "Choose where to look" screen that explains the prompts and
      walks only after a click. "Where your space goes" measures only on the Measure button. Smart Scan
      only on Scan, with the explanation shown before the first scan.
    - Quick Clean (menu bar) scans only Safe categories; a unit test asserts none of them walks `$HOME` or
      touches a consent-gated folder.
    - The FDA probe lists only FDA-gated folders (Safari, Mail, Messages, the system TCC folder), which
      are silently denied without FDA and never prompt.
    - Home walks skip app-managed library packages and `Media.localized` (Music/TV media).
14. **Concurrency.** Each Smart Scan, Quick Clean preview and Explorer navigation gets its own cancel token;
    starting a new one trips the previous token (an old walk can never be "un-cancelled"). A superseded
    scan stops emitting events. Explorer listings return at most 2000 entries (largest first) plus a count
    of the rest; the UI renders 300 rows (200 per Smart Clean category) before "Show more".
15. **Sizes** are allocated blocks (`st_blocks × 512`), not logical length, so sparse files count what they
    occupy. The Explorer counts each hard-linked inode once and never crosses devices; listing `/` also
    follows the firmlinks into the Data volume (`/Users`, `/Applications`…) and sizes each child on its own
    device, so the Data volume is not counted twice. Known limit: APFS clones share blocks but are counted
    in full, so "freed" can overstate what deleting a cloned file returns.

## Failure modes
| Risk | Mitigation |
|---|---|
| Deleting user data | guard (roots, subtrees, packages, git roots) + Review/Trash defaults + confirm with exact totals |
| Mislabelled build dir | artifact rules require manifest + marker evidence; test covers lookalikes |
| Shell injection via tool args | fixed tool ids; snapshot date strict format + must be listed by tmutil |
| App holds files open | per-item errors collected, logged, never abort the batch |
| Huge trees stall UI | all work off the main thread; streaming events; cancel |
| Alert spam | hysteresis + cooldown + snooze |
| Stale scan | items re-validated at clean time; Quick Clean preview expires after 10 min |
| DB corruption/unavailable | a non-SQLite/corrupt file is moved aside (`tidy.db.corrupt-<ts>`) and a fresh DB created; if it still can't open, the app runs without history; migrations are transactional |
| Scheduler after sleep | due checks use wall-clock time, so a check runs right after wake |
| Settings torn by a crash | temp file + fsync + rename; unreadable settings fall back to defaults |
| Privacy prompt at launch | no folder walks at launch / in background / in the popover (decision 13) |

## Windows and materials
The main window is a normal opaque window (an experimental "liquid glass" vibrancy was removed: no visible
benefit in the real app). The menu bar popover is the only window with a native material
(`Effect::Popover`, transparent webview background, needs `macOSPrivateApi`). The popover shows the top 3
space users so status, Quick Clean, alerts and Open/Quit fit in its 340×560 window.

## Security
Tauri capabilities grant the webviews only window drag/hide/set-theme on top of `core:default`;
notifications and launch-at-login are driven from Rust, so their plugins expose nothing to JavaScript.
Subprocesses are spawned with argument vectors (no shell), except the admin snapshot deletion, which
passes one fixed `tmutil` command with a strictly validated date to `osascript … with administrator
privileges`. `reveal` accepts only absolute paths (an argument starting with `-` would be an option).

## Verified only by tests / headless rendering (not in a real GUI session)
Popover position and material on multi-monitor setups, tray badge legibility in light and dark menu bars,
menu-bar-only mode (Dock icon hiding), the admin password dialog for snapshot deletion, Full Disk Access
grant/re-check flows, notification delivery, and the absence of privacy prompts at launch.

## Not built yet (and why)
- **App uninstaller with leftovers**: needs deletion outside `$HOME` (`/Applications`) and bundle-id matching
  of leftovers; must be designed with its own guard rules first.
- **Duplicate finder**: planned as size → partial hash → full hash with a hash cache in the store.
- **Treemap/sunburst**: the Explorer's sized list + space breakdown cover the need for now.
- **Notification click → page**: the notification plugin exposes no click callback on desktop; instead the
  next time the window gains focus within 30 min of an alert, it opens Smart Clean.
- **Global shortcut**: would need another plugin; left out.

## Roadmap
v2: duplicate finder, app uninstaller, scheduled auto-clean of Safe items. v3: Windows (`platform/windows.rs`), Linux.
