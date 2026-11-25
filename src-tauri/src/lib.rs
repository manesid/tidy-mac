mod engine;
mod platform;

use engine::{
    alerts::{self, AlertState, Level},
    catalog::{self, Category, Disposal, Safety},
    cleaner::{self, Job, Report},
    disk::{self, DiskUsage},
    explorer::{self, Insights, Listing},
    menubar::{self, fmt_bytes, Badge, Snooze},
    scanner,
    settings::Settings,
    space::{self, Hog},
    store::{HistoryRow, NewHistory, Sample, Store},
    trend::{self, Forecast},
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    window::{Effect, EffectState, EffectsBuilder},
    ActivationPolicy, AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, State, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt;

const DAY: u64 = 86_400;
/// A Quick Clean preview must be confirmed within this window, otherwise it is re-scanned.
const QUICK_PREVIEW_TTL: u64 = 10 * 60;
/// After a storage alert, the next time Tidy's window is opened it lands on Smart Clean.
const ALERT_NAV_WINDOW: u64 = 30 * 60;
const SPACE_KEY: &str = "space_breakdown";
const POPOVER: &str = "popover";
const POPOVER_W: f64 = 340.0;
const POPOVER_H: f64 = 560.0;

struct Stored {
    path: PathBuf,
    size: u64,
    disposal: Disposal,
}

struct QuickPreview {
    ids: Vec<String>,
    at: u64,
}

struct AppState {
    home: PathBuf,
    settings_path: PathBuf,
    settings: Mutex<Settings>,
    alert: Mutex<AlertState>,
    store: Mutex<HashMap<String, Stored>>,
    /// SQLite history. `None` if the database could not be opened: the app still works without it.
    db: Option<Store>,
    quick: Mutex<Option<QuickPreview>>,
    /// Bumped by every Smart Scan; item ids embed it, so ids from an older scan never resolve to
    /// items of a newer one, and events from a superseded scan are dropped.
    scan_gen: AtomicU64,
    /// Cancel token of the running Smart Scan / Quick Clean preview / Explorer walk. Starting a new one
    /// swaps in a fresh token and trips the old one, so an old walk can never be "un-cancelled".
    scan_cancel: Mutex<Arc<AtomicBool>>,
    quick_cancel: Mutex<Arc<AtomicBool>>,
    explorer_cancel: Mutex<Arc<AtomicBool>>,
    /// Paths shown in the current Explorer listing: the only paths `explorer_trash` accepts.
    explorer_shown: Mutex<HashSet<PathBuf>>,
    /// Last tray menu inputs, so the menu is rebuilt only when its text changes.
    tray_menu_key: Mutex<Option<(String, bool)>>,
    tray_badge: Mutex<Option<Badge>>,
    alert_nav_at: Mutex<Option<u64>>,
    /// When the popover last hid on blur: a tray click right after that is a "close", not "reopen".
    popover_hidden_at: Mutex<Option<Instant>>,
}

#[derive(Serialize, Clone)]
struct ItemDto {
    id: String,
    name: String,
    path: String,
    size: u64,
}

#[derive(Serialize, Clone)]
struct CategoryDto {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    group: &'static str,
    safety: Safety,
    disposal: Disposal,
    needs_fda: bool,
    blocked: bool,
    total: u64,
    items: Vec<ItemDto>,
}

/// Trip the current token and install a fresh one; returns the fresh token.
fn renew(slot: &Mutex<Arc<AtomicBool>>) -> Arc<AtomicBool> {
    let fresh = Arc::new(AtomicBool::new(false));
    let old = std::mem::replace(&mut *slot.lock(), fresh.clone());
    old.store(true, Ordering::Relaxed);
    fresh
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn disposal_str(d: Disposal) -> &'static str {
    match d {
        Disposal::Delete => "delete",
        Disposal::Trash => "trash",
        Disposal::EmptyTrash => "emptytrash",
    }
}

/// Run blocking work (filesystem, SQLite, subprocesses) off the UI thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

// ---------- scanning & cleaning ----------

/// Scan one category; item ids are `<prefix><category>:<n>` and resolve only against this scan.
/// Categories that need Full Disk Access are not read at all without it (reported as blocked), so a
/// scan never pokes at privacy-protected locations the user has not granted.
fn scan_category(st: &AppState, cat: &Category, prefix: &str, cancel: &AtomicBool, fda: bool) -> CategoryDto {
    let out = if cat.needs_fda && !fda { scanner::Outcome { items: vec![], blocked: true } } else { scanner::scan(cat, &st.home, cancel) };
    let mut store = st.store.lock();
    let items: Vec<ItemDto> = out
        .items
        .into_iter()
        .enumerate()
        .map(|(i, f)| {
            let id = format!("{prefix}{}:{}", cat.id, i);
            store.insert(id.clone(), Stored { path: f.path.clone(), size: f.size, disposal: cat.disposal });
            ItemDto { id, name: f.name, path: f.path.display().to_string(), size: f.size }
        })
        .collect();
    CategoryDto {
        id: cat.id,
        name: cat.name,
        description: cat.description,
        group: cat.group,
        safety: cat.safety,
        disposal: cat.disposal,
        needs_fda: cat.needs_fda,
        blocked: out.blocked,
        total: items.iter().map(|i| i.size).sum(),
        items,
    }
}

fn log_report(st: &AppState, source: &str, r: &Report) {
    let Some(db) = &st.db else { return };
    let ok = r.done.iter().map(|(p, size, d)| (p.display().to_string(), *size, disposal_str(*d), true, None));
    let failed = r.failed.iter().map(|f| (f.path.clone(), 0, "delete", false, Some(f.reason.clone())));
    let rows: Vec<_> = ok.chain(failed).collect();
    let rows: Vec<NewHistory> = rows
        .iter()
        .map(|(path, size, disposal, ok, reason)| NewHistory { source, path, size: *size, disposal, ok: *ok, reason: reason.as_deref() })
        .collect();
    let _ = db.log(now(), &rows);
}

fn run_jobs(app: &AppHandle, jobs: Vec<Job>, source: &str) -> Report {
    let st = app.state::<AppState>();
    let report = cleaner::run(jobs, &st.home, platform::empty_trash, platform::trash);
    log_report(&st, source, &report);
    {
        let mut s = st.settings.lock();
        s.lifetime_freed += report.freed;
        let _ = s.save(&st.settings_path);
    }
    refresh(app, false);
    report
}

fn clean_ids(app: &AppHandle, ids: Vec<String>, source: &str) -> Report {
    let st = app.state::<AppState>();
    let jobs: Vec<Job> = {
        let mut store = st.store.lock();
        ids.iter().filter_map(|id| store.remove(id)).map(|s| Job { path: s.path, size: s.size, disposal: s.disposal }).collect()
    };
    run_jobs(app, jobs, source)
}

#[tauri::command]
fn scan_start(app: AppHandle, st: State<'_, AppState>) {
    let cancel = renew(&st.scan_cancel); // stops a scan that is still running
    let gen = st.scan_gen.fetch_add(1, Ordering::SeqCst) + 1;
    st.store.lock().retain(|k, _| k.starts_with("q/")); // keep a pending Quick Clean preview valid
    std::thread::spawn(move || {
        let st = app.state::<AppState>();
        let current = || st.scan_gen.load(Ordering::SeqCst) == gen && !cancel.load(Ordering::Relaxed);
        let fda = platform::has_full_disk_access();
        for cat in catalog::build(&st.home) {
            if !current() {
                return;
            }
            let _ = app.emit("scan:progress", cat.name);
            let dto = scan_category(&st, &cat, &format!("s{gen}/"), &cancel, fda);
            if !current() {
                return; // superseded or stopped: a partial category must not be shown as complete
            }
            let _ = app.emit("scan:category", dto);
        }
        let _ = app.emit("scan:done", ());
    });
}

#[tauri::command]
fn scan_cancel(st: State<'_, AppState>) {
    renew(&st.scan_cancel);
}

#[tauri::command]
async fn clean(app: AppHandle, ids: Vec<String>) -> Result<Report, String> {
    blocking(move || clean_ids(&app, ids.into_iter().filter(|i| !i.starts_with("q/")).collect(), "smart")).await
}

#[derive(Serialize)]
struct QuickPreviewDto {
    total: u64,
    count: usize,
    categories: Vec<(String, u64)>,
}

/// Menu-bar Quick Clean, step 1: scan only Safe categories and report exactly what would go.
/// Nothing is removed until `quick_clean_commit` is called from a button showing this total.
#[tauri::command]
async fn quick_preview(app: AppHandle) -> Result<QuickPreviewDto, String> {
    blocking(move || {
        let st = app.state::<AppState>();
        let cancel = renew(&st.quick_cancel);
        st.store.lock().retain(|k, _| !k.starts_with("q/"));
        *st.quick.lock() = None;
        let fda = platform::has_full_disk_access();
        let (mut ids, mut categories, mut total) = (Vec::new(), Vec::new(), 0);
        for cat in catalog::build(&st.home).into_iter().filter(|c| c.safety == Safety::Safe) {
            let dto = scan_category(&st, &cat, "q/", &cancel, fda);
            if dto.total > 0 {
                categories.push((dto.name.to_string(), dto.total));
            }
            total += dto.total;
            ids.extend(dto.items.into_iter().map(|i| i.id));
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("Superseded by a newer check".to_string());
        }
        categories.sort_by_key(|c| std::cmp::Reverse(c.1));
        let count = ids.len();
        *st.quick.lock() = Some(QuickPreview { ids, at: now() });
        Ok(QuickPreviewDto { total, count, categories })
    })
    .await?
}

/// Step 2: clean exactly the previewed items (re-checked by the guard at delete time).
#[tauri::command]
async fn quick_clean_commit(app: AppHandle) -> Result<Report, String> {
    blocking(move || {
        let st = app.state::<AppState>();
        let preview = st.quick.lock().take().ok_or("Preview first")?;
        if now() > preview.at + QUICK_PREVIEW_TTL {
            return Err("Preview expired, check again".to_string());
        }
        Ok(clean_ids(&app, preview.ids, "quick"))
    })
    .await?
}

// ---------- explorer (read-only, any readable path) ----------

fn checked_dir(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    if !p.is_absolute() || !p.is_dir() {
        return Err("not an absolute directory".into());
    }
    Ok(p)
}

/// `/` on macOS reaches the Data volume through firmlinks (/Users, /Applications, ...): allow that device.
fn extra_devs(p: &Path) -> Vec<u64> {
    if p == Path::new("/") { vec![explorer::dev_of(Path::new(platform::DATA_VOLUME))] } else { vec![] }
}

// PRIVACY: Explorer walks only run on an explicit user action in the Explorer page (or a click on a
// "where your space goes" row). Nothing calls them at launch, from the scheduler or from the popover,
// because walking ~/Desktop, ~/Documents, ~/Music, ~/Pictures... triggers macOS consent prompts.
#[tauri::command]
async fn explorer_list(app: AppHandle, path: String) -> Result<Listing, String> {
    let p = checked_dir(&path)?;
    let cancel = app.state::<AppState>().explorer_cancel.lock().clone();
    blocking(move || {
        let mut l = explorer::list(&p, &extra_devs(&p), &cancel);
        let st = app.state::<AppState>();
        if !cancel.load(Ordering::Relaxed) {
            st.explorer_shown.lock().extend(l.entries.iter().map(|e| e.path.clone()));
        }
        // Only complete listings become baselines; a cancelled walk would record false shrinkage.
        if let (Some(db), false, false) = (&st.db, l.unreadable, cancel.load(Ordering::Relaxed)) {
            let sizes: Vec<(String, u64)> = l.entries.iter().map(|e| (e.name.clone(), e.size)).collect();
            if let Ok(Some(base)) = db.snapshot(&p.display().to_string(), now(), &sizes) {
                explorer::apply_baseline(&mut l, base.ts, &base.sizes);
            }
        }
        l
    })
    .await
}

#[tauri::command]
async fn explorer_insights(st: State<'_, AppState>, path: String) -> Result<Insights, String> {
    let p = checked_dir(&path)?;
    let cancel = st.explorer_cancel.lock().clone();
    blocking(move || explorer::insights(&p, 30, &extra_devs(&p), &cancel)).await
}

/// Stops running Explorer walks and starts a new navigation (fresh token, nothing trashable yet).
#[tauri::command]
fn explorer_cancel(st: State<'_, AppState>) {
    renew(&st.explorer_cancel);
    st.explorer_shown.lock().clear();
}

/// Move one explorer entry to Trash. The path must be one the current Explorer listing showed (the webview
/// cannot name arbitrary paths), and it then passes the same guard as the cleaner.
#[tauri::command]
async fn explorer_trash(app: AppHandle, path: String) -> Result<Report, String> {
    blocking(move || {
        let p = PathBuf::from(&path);
        if !app.state::<AppState>().explorer_shown.lock().remove(&p) {
            return Err("This item is no longer in the current listing. Refresh and try again.".to_string());
        }
        let size = scanner::size_of(&p, &AtomicBool::new(false));
        Ok(run_jobs(&app, vec![Job { path: p, size, disposal: Disposal::Trash }], "explorer"))
    })
    .await?
}

#[tauri::command]
fn reveal(path: String) {
    platform::reveal(Path::new(&path));
}

// ---------- insight: history, trend, space breakdown ----------

#[derive(Serialize)]
struct UsageHistory {
    samples: Vec<Sample>,
    forecast: Option<Forecast>,
}

fn forecast_now(db: &Store) -> Option<Forecast> {
    let t = now();
    let s = db.usage_since(t.saturating_sub(14 * DAY)).ok()?;
    trend::forecast(&s, t, 14 * DAY, 2 * DAY)
}

#[tauri::command]
async fn usage_history(app: AppHandle, days: u64) -> Result<UsageHistory, String> {
    blocking(move || {
        let st = app.state::<AppState>();
        let Some(db) = &st.db else { return UsageHistory { samples: vec![], forecast: None } };
        let samples = db.usage_since(now().saturating_sub(days.clamp(1, 730) * DAY)).unwrap_or_default();
        UsageHistory { samples, forecast: forecast_now(db) }
    })
    .await
}

#[tauri::command]
async fn history_list(app: AppHandle) -> Result<Vec<HistoryRow>, String> {
    blocking(move || app.state::<AppState>().db.as_ref().map(|d| d.history(500).unwrap_or_default()).unwrap_or_default()).await
}

#[derive(Serialize)]
struct SpaceBreakdown {
    hogs: Vec<Hog>,
    measured_at: Option<u64>,
    local_snapshots: usize,
}

fn cached_space(st: &AppState) -> Option<(Vec<Hog>, u64)> {
    st.db.as_ref()?.get_json::<Vec<Hog>>(SPACE_KEY)
}

/// `refresh = false` returns the cached breakdown (instant); `true` re-measures (seconds to minutes).
#[tauri::command]
async fn space_breakdown(app: AppHandle, refresh: bool) -> Result<SpaceBreakdown, String> {
    blocking(move || {
        let st = app.state::<AppState>();
        let local_snapshots = platform::tm_snapshots().len();
        if !refresh {
            let c = cached_space(&st);
            return SpaceBreakdown { measured_at: c.as_ref().map(|c| c.1), hogs: c.map(|c| c.0).unwrap_or_default(), local_snapshots };
        }
        let hogs = space::measure(platform::space_locations(&st.home), &AtomicBool::new(false));
        let t = now();
        if let Some(db) = &st.db {
            let _ = db.put_json(SPACE_KEY, t, &hogs);
        }
        SpaceBreakdown { hogs, measured_at: Some(t), local_snapshots }
    })
    .await
}

// ---------- maintenance tools (official CLIs, fixed ids only) ----------

#[derive(Serialize)]
struct Tool {
    id: &'static str,
    available: bool,
    /// Exact command Tidy runs, shown to the user before they confirm.
    command: String,
    needs_admin: bool,
    /// Human summary of what was found, e.g. "3 devices · 1.2 GB".
    found: String,
    bytes: Option<u64>,
    /// Extra arguments the user can pick from (snapshot dates).
    choices: Vec<String>,
}

#[tauri::command]
async fn tools_status() -> Result<Vec<Tool>, String> {
    blocking(|| {
        let snaps = platform::tm_snapshots();
        let sims = platform::unavailable_sims();
        let brew = platform::brew_preview();
        vec![
            Tool {
                id: "tm-snapshots",
                available: true,
                command: platform::tm_delete_command("<date>"),
                needs_admin: true,
                found: match snaps.len() { 0 => "No local snapshots".into(), 1 => "1 local snapshot".into(), n => format!("{n} local snapshots") },
                bytes: None,
                choices: snaps,
            },
            Tool {
                id: "simulators",
                available: sims.is_some(),
                command: platform::SIMS_DELETE_COMMAND.into(),
                needs_admin: false,
                found: match sims { None => "Xcode not installed".into(), Some((0, _)) => "No unavailable simulators".into(), Some((n, b)) => format!("{n} unavailable · {}", fmt_bytes(b)) },
                bytes: sims.map(|s| s.1),
                choices: vec![],
            },
            Tool {
                id: "homebrew",
                available: brew.is_some(),
                command: platform::BREW_CLEANUP_COMMAND.into(),
                needs_admin: false,
                found: match &brew {
                    None => "Homebrew not installed".into(),
                    Some((0, _)) => "Nothing to clean".into(),
                    Some((n, est)) => format!("{n} old items{}", est.as_ref().map(|e| format!(" · about {e}")).unwrap_or_default()),
                },
                bytes: None,
                choices: vec![],
            },
        ]
    })
    .await
}

#[tauri::command]
async fn tool_run(app: AppHandle, id: String, arg: Option<String>) -> Result<(), String> {
    blocking(move || {
        let (command, res, bytes) = match id.as_str() {
            "tm-snapshots" => {
                let date = arg.ok_or("pick a snapshot")?;
                // Only dates that tmutil itself just listed are accepted.
                if !platform::tm_snapshots().contains(&date) {
                    return Err("snapshot not found".to_string());
                }
                (platform::tm_delete_command(&date), platform::tm_delete(&date), 0)
            }
            "simulators" => {
                let b = platform::unavailable_sims().map(|s| s.1).unwrap_or(0);
                (platform::SIMS_DELETE_COMMAND.to_string(), platform::delete_unavailable_sims(), b)
            }
            "homebrew" => (platform::BREW_CLEANUP_COMMAND.to_string(), platform::brew_cleanup(), 0),
            _ => return Err("unknown tool".to_string()),
        };
        let st = app.state::<AppState>();
        if let Some(db) = &st.db {
            let reason = res.as_ref().err().map(String::as_str);
            let _ = db.log(now(), &[NewHistory { source: "tool", path: &command, size: bytes, disposal: "command", ok: res.is_ok(), reason }]);
        }
        refresh(&app, false);
        res
    })
    .await?
}

// ---------- system ----------

#[derive(Serialize)]
struct Overview {
    disk: Option<DiskUsage>,
    full_disk_access: bool,
    lifetime_freed: u64,
    home: String,
    volume: &'static str,
    history_available: bool,
}

#[tauri::command]
fn overview(st: State<'_, AppState>) -> Overview {
    Overview {
        disk: disk::usage(platform::DATA_VOLUME),
        full_disk_access: platform::has_full_disk_access(),
        lifetime_freed: st.settings.lock().lifetime_freed,
        home: st.home.display().to_string(),
        volume: platform::DATA_VOLUME,
        history_available: st.db.is_some(),
    }
}

#[tauri::command]
fn open_full_disk_access(app: AppHandle) {
    use tauri_plugin_opener::OpenerExt;
    let _ = app.opener().open_url(platform::FDA_SETTINGS_URL, None::<&str>);
}

#[tauri::command]
fn settings_get(st: State<'_, AppState>) -> Settings {
    st.settings.lock().clone()
}

fn apply_activation_policy(app: &AppHandle, menu_bar_only: bool) {
    let _ = app.set_activation_policy(if menu_bar_only { ActivationPolicy::Accessory } else { ActivationPolicy::Regular });
}

#[tauri::command]
fn settings_set(app: AppHandle, st: State<'_, AppState>, settings: Settings) -> Settings {
    let mut new = settings.sanitized();
    let mut cur = st.settings.lock();
    new.lifetime_freed = cur.lifetime_freed; // not UI-writable
    new.snoozed_until = cur.snoozed_until; // changed only through `snooze`
    if new.launch_at_login != cur.launch_at_login {
        let al = app.autolaunch();
        let _ = if new.launch_at_login { al.enable() } else { al.disable() };
        // Report what actually happened, not what was asked (enabling can fail).
        if let Ok(on) = al.is_enabled() {
            new.launch_at_login = on;
        }
    }
    let policy_changed = new.menu_bar_only != cur.menu_bar_only;
    let _ = new.save(&st.settings_path);
    *cur = new.clone();
    drop(cur);
    if policy_changed {
        apply_activation_policy(&app, new.menu_bar_only);
        show_main(&app); // switching policy can hide the window; keep it in front
    }
    let _ = app.emit("settings:changed", ());
    refresh(&app, false);
    new
}

/// Mute alerts for an hour / until tomorrow 08:00 local; `None` un-snoozes.
#[tauri::command]
fn snooze(app: AppHandle, st: State<'_, AppState>, until: Option<Snooze>) -> Settings {
    let t = now();
    let s = {
        let mut s = st.settings.lock();
        s.snoozed_until = until.map_or(0, |k| menubar::snooze_until(k, t, platform::utc_offset(t)));
        let _ = s.save(&st.settings_path);
        s.clone()
    };
    let _ = app.emit("settings:changed", ());
    refresh(&app, false);
    s
}

// ---------- menu bar popover ----------

#[derive(Serialize)]
struct PopoverState {
    disk: Option<DiskUsage>,
    level: Level,
    forecast: Option<Forecast>,
    hogs: Vec<Hog>,
    hogs_at: Option<u64>,
    settings: Settings,
    full_disk_access: bool,
}

fn level_now(s: &Settings, u: Option<DiskUsage>) -> Level {
    match u {
        Some(u) if u.used_pct >= s.critical_pct as f32 => Level::Critical,
        Some(u) if u.used_pct >= s.warn_pct as f32 => Level::Warn,
        _ => Level::Ok,
    }
}

#[tauri::command]
async fn popover_state(app: AppHandle) -> Result<PopoverState, String> {
    blocking(move || {
        let st = app.state::<AppState>();
        let settings = st.settings.lock().clone();
        let disk = disk::usage(platform::DATA_VOLUME);
        let cached = cached_space(&st);
        PopoverState {
            level: level_now(&settings, disk),
            disk,
            forecast: st.db.as_ref().and_then(forecast_now),
            hogs_at: cached.as_ref().map(|c| c.1),
            hogs: cached.map(|c| c.0.into_iter().take(3).collect()).unwrap_or_default(),
            settings,
            full_disk_access: platform::has_full_disk_access(),
        }
    })
    .await
}

/// Open the main window on a given page (from the popover).
#[tauri::command]
fn open_main(app: AppHandle, page: Option<String>) {
    hide_popover(&app);
    show_main(&app);
    if let Some(p) = page {
        let _ = app.emit_to("main", "nav", p);
    }
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

fn hide_popover(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(POPOVER) {
        let _ = w.hide();
    }
}

fn toggle_popover(app: &AppHandle, rect: tauri::Rect) {
    let Some(w) = app.get_webview_window(POPOVER) else { return };
    let just_hidden = app.state::<AppState>().popover_hidden_at.lock().take().is_some_and(|t| t.elapsed() < Duration::from_millis(300));
    if w.is_visible().unwrap_or(false) || just_hidden {
        let _ = w.hide();
        return;
    }
    let scale = w.scale_factor().unwrap_or(2.0);
    let pos = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    let width = POPOVER_W * scale;
    let mut x = pos.x + size.width / 2.0 - width / 2.0;
    // keep fully on the screen that holds the menu bar icon
    if let Ok(Some(m)) = app.monitor_from_point(pos.x, pos.y) {
        let (mx, mw) = (m.position().x as f64, m.size().width as f64);
        x = x.clamp(mx + 8.0 * scale, mx + mw - width - 8.0 * scale);
    }
    let _ = w.set_position(PhysicalPosition::new(x, pos.y + size.height + 4.0 * scale));
    let _ = w.emit("popover:open", ());
    let _ = w.show();
    let _ = w.set_focus();
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

fn build_menu(app: &AppHandle, usage: Option<DiskUsage>, alerts_on: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let header = match usage {
        Some(u) => format!("{} free of {}  ·  {:.0}% used", fmt_bytes(u.free), fmt_bytes(u.total), u.used_pct),
        None => "Disk usage unavailable".into(),
    };
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "hdr", header, false, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "scan", "Smart Scan…", true, None::<&str>)?,
            &MenuItem::with_id(app, "explore", "Explore Storage…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &CheckMenuItem::with_id(app, "alerts", "Storage alerts", true, alerts_on, None::<&str>)?,
            &MenuItem::with_id(app, "check", "Check now", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open", "Open Tidy", true, None::<&str>)?,
            &MenuItem::with_id(app, "quit", "Quit Tidy", true, Some("Cmd+Q"))?,
        ],
    )
}

fn tray_icon(badge: Badge) -> Option<Image<'static>> {
    let base = Image::from_bytes(include_bytes!("../icons/tray.png")).ok()?;
    let (w, h) = (base.width(), base.height());
    let mut rgba = base.rgba().to_vec();
    menubar::draw_badge(&mut rgba, w, h, badge);
    Some(Image::new_owned(rgba, w, h))
}

/// Re-read disk usage, update UI + tray, optionally evaluate alerts.
fn refresh(app: &AppHandle, check_alerts: bool) {
    let st = app.state::<AppState>();
    let usage = disk::usage(platform::DATA_VOLUME);
    let s = st.settings.lock().clone();
    let t = now();
    let _ = app.emit("disk:update", usage);
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_title(menubar::tray_title(s.tray_title, usage.map(|u| u.free), usage.map(|u| u.used_pct)));
        let badge = menubar::badge_for(level_now(&s, usage));
        let mut last = st.tray_badge.lock();
        if *last != Some(badge) {
            if let Some(img) = tray_icon(badge) {
                let _ = tray.set_icon(Some(img));
                let _ = tray.set_icon_as_template(true);
            }
            *last = Some(badge);
        }
        // Rebuild the menu only when its text changes (the scheduler calls this every 30 s).
        let key = (usage.map(|u| format!("{}/{}/{:.0}", fmt_bytes(u.free), fmt_bytes(u.total), u.used_pct)).unwrap_or_default(), s.alerts_enabled);
        let mut last_menu = st.tray_menu_key.lock();
        if last_menu.as_ref() != Some(&key) {
            if let Ok(m) = build_menu(app, usage, s.alerts_enabled) {
                let _ = tray.set_menu(Some(m));
                *last_menu = Some(key);
            }
        }
    }
    let snoozed = menubar::is_snoozed(s.snoozed_until, t);
    if let (true, true, false, Some(u)) = (check_alerts, s.alerts_enabled, snoozed, usage) {
        let (next, fire) = alerts::step(*st.alert.lock(), u.used_pct, s.warn_pct, s.critical_pct, t);
        *st.alert.lock() = next;
        if let Some(level) = fire {
            let (title, tail) = match level {
                Level::Critical => ("Disk almost full", "Open Tidy to review what can be removed."),
                _ => ("Storage is getting full", "Open Tidy to run a Smart Scan."),
            };
            *st.alert_nav_at.lock() = Some(t);
            notify(app, title, &format!("{} free ({:.0}% used). {tail}", fmt_bytes(u.free), u.used_pct));
        }
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id("main")
        .icon(tray_icon(Badge::None).ok_or(tauri::Error::InvalidIcon(std::io::Error::other("tray icon")))?)
        .icon_as_template(true)
        .tooltip("Tidy")
        .show_menu_on_left_click(false) // left click: popover; right click: menu
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = ev {
                toggle_popover(tray.app_handle(), rect);
            }
        })
        .on_menu_event(|app, ev| match ev.id.as_ref() {
            "open" => show_main(app),
            "scan" | "explore" => {
                show_main(app);
                let _ = app.emit_to("main", "nav", ev.id.as_ref());
            }
            "check" => refresh(app, true),
            "alerts" => {
                let st = app.state::<AppState>();
                {
                    let mut s = st.settings.lock();
                    s.alerts_enabled = !s.alerts_enabled;
                    let _ = s.save(&st.settings_path);
                }
                let _ = app.emit("settings:changed", ());
                refresh(app, false);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    refresh(app, false);
    Ok(())
}

fn setup_popover(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, POPOVER, WebviewUrl::App("index.html".into()))
        .title("Tidy")
        .inner_size(POPOVER_W, POPOVER_H)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .shadow(true)
        // Native popover material with rounded corners, like system menu bar extras.
        .transparent(true)
        .effects(EffectsBuilder::new().effect(Effect::Popover).state(EffectState::Active).radius(12.0).build())
        .accept_first_mouse(true)
        .visible(false)
        .build()?;
    Ok(())
}

/// Background loop: statvfs + one SQLite write at most every 15 min. PRIVACY: it never walks folders.
/// Uses wall-clock time (not `Instant`, which stops while the Mac sleeps) so checks are due right after wake.
fn start_scheduler(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_alert_check: Option<u64> = None;
        let mut last_prune = 0u64;
        loop {
            let st = app.state::<AppState>();
            let every = st.settings.lock().check_minutes as u64 * 60;
            let t = now();
            // `t < last` covers the clock being set backwards.
            let due = last_alert_check.is_none_or(|last| t < last || t >= last + every);
            if due {
                last_alert_check = Some(t);
            }
            if let (Some(db), Some(u)) = (&st.db, disk::usage(platform::DATA_VOLUME)) {
                let t = now();
                let _ = db.record_usage(t, u.used, u.total);
                if t >= last_prune + DAY {
                    let _ = db.prune(t);
                    last_prune = t;
                }
            }
            refresh(&app, due);
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![])))
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            let settings_path = dir.join("settings.json");
            let settings = Settings::load(&settings_path);
            let menu_bar_only = settings.menu_bar_only;
            let db = Store::open(&dir.join("tidy.db")).map_err(|e| eprintln!("history disabled: {e}")).ok();
            app.manage(AppState {
                home: platform::home(),
                settings: Mutex::new(settings),
                settings_path,
                alert: Mutex::new(AlertState::default()),
                store: Mutex::new(HashMap::new()),
                db,
                quick: Mutex::new(None),
                scan_gen: AtomicU64::new(0),
                scan_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
                quick_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
                explorer_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
                explorer_shown: Mutex::new(HashSet::new()),
                tray_menu_key: Mutex::new(None),
                tray_badge: Mutex::new(None),
                alert_nav_at: Mutex::new(None),
                popover_hidden_at: Mutex::new(None),
            });
            if menu_bar_only {
                app.set_activation_policy(ActivationPolicy::Accessory);
            }
            let _ = app.notification().request_permission();
            setup_popover(app.handle())?;
            setup_tray(app.handle())?;
            start_scheduler(app.handle().clone());
            Ok(())
        })
        .on_window_event(|w, ev| match ev {
            // Closing the main window keeps Tidy alive in the menu bar so alerts keep working.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = w.hide();
            }
            // The popover behaves like a menu: it goes away when it loses focus.
            WindowEvent::Focused(false) if w.label() == POPOVER => {
                if w.is_visible().unwrap_or(false) {
                    *w.state::<AppState>().popover_hidden_at.lock() = Some(Instant::now());
                }
                let _ = w.hide();
            }
            // Opening Tidy shortly after a storage alert lands on Smart Clean.
            WindowEvent::Focused(true) if w.label() == "main" => {
                let st = w.state::<AppState>();
                let at = st.alert_nav_at.lock().take();
                if at.is_some_and(|t| now() <= t + ALERT_NAV_WINDOW) {
                    let _ = w.emit("nav", "scan");
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            overview, scan_start, scan_cancel, clean, quick_preview, quick_clean_commit, explorer_list, explorer_insights,
            explorer_cancel, explorer_trash, reveal, open_full_disk_access, settings_get, settings_set, snooze, usage_history,
            history_list, space_breakdown, tools_status, tool_run, popover_state, open_main, quit
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Tidy");
    app.run(|app, ev| {
        if let RunEvent::Reopen { .. } = ev {
            show_main(app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renew_trips_the_old_token_and_returns_a_live_one() {
        let slot = Mutex::new(Arc::new(AtomicBool::new(false)));
        let old = slot.lock().clone();
        let fresh = renew(&slot);
        assert!(old.load(Ordering::Relaxed), "a superseded walk stops");
        assert!(!fresh.load(Ordering::Relaxed));
        assert!(Arc::ptr_eq(&fresh, &slot.lock()));
        // cancelling again never revives the old walk
        renew(&slot);
        assert!(old.load(Ordering::Relaxed) && fresh.load(Ordering::Relaxed));
    }
}
