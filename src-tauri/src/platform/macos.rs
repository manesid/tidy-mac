use std::path::{Path, PathBuf};
use std::process::Command;

pub fn home() -> PathBuf {
    dirs::home_dir().expect("no home directory")
}

/// Volume whose usage we report (APFS Data volume holds user space).
pub const DATA_VOLUME: &str = "/System/Volumes/Data";

/// Full Disk Access probe. PRIVACY INVARIANT: it only lists folders that are gated by Full Disk Access
/// alone (Safari, Mail, Messages, the system TCC folder). macOS never shows a consent prompt for those;
/// access is silently denied without FDA. Never add Desktop/Documents/Downloads, Pictures/Photos,
/// Music/Media, iCloud Drive or other apps' containers here: those DO prompt.
pub fn has_full_disk_access() -> bool {
    let h = home();
    [h.join("Library/Safari"), h.join("Library/Mail"), h.join("Library/Messages")]
        .iter()
        .filter(|p| p.exists())
        .any(|p| std::fs::read_dir(p).is_ok())
        || !h.join("Library/Safari").exists() && std::fs::read_dir("/Library/Application Support/com.apple.TCC").is_ok()
}

pub const FDA_SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// Rename that fails instead of replacing an existing destination (no check-then-rename race).
fn rename_excl(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &Path| std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(|_| std::io::Error::other("path contains NUL"));
    let (f, t) = (c(from)?, c(to)?);
    if unsafe { libc::renamex_np(f.as_ptr(), t.as_ptr(), libc::RENAME_EXCL) } == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// Move into `trash_dir` under a free name ("a.dmg", "a 2.dmg", ...), never overwriting anything there.
/// Only a same-volume rename is attempted: an item on another volume fails with a clear error instead
/// of being copied (Tidy's guard keeps targets inside $HOME, which lives on the same volume as ~/.Trash).
pub fn trash_into(path: &Path, trash_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(trash_dir)?;
    let name = path.file_name().ok_or_else(|| std::io::Error::other("no file name"))?;
    let p = Path::new(name);
    let (stem, ext) = match (p.file_stem(), p.extension()) {
        (Some(s), Some(e)) if !s.is_empty() => (s.to_string_lossy().into_owned(), format!(".{}", e.to_string_lossy())),
        _ => (name.to_string_lossy().into_owned(), String::new()),
    };
    for n in 1..10_000 {
        let dest = if n == 1 { trash_dir.join(name) } else { trash_dir.join(format!("{stem} {n}{ext}")) };
        match rename_excl(path, &dest) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => return Err(std::io::Error::other("on another volume; move it to the Trash in Finder")),
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("too many items with this name in the Trash"))
}

pub fn trash(path: &Path) -> std::io::Result<()> {
    trash_into(path, &home().join(".Trash"))
}

/// Empty ~/.Trash. Fails (rather than reporting success) if any item could not be removed.
/// No Finder/AppleScript fallback: that would need Automation permission and empties the Trash of
/// every volume, which is more than the size the user confirmed.
pub fn empty_trash() -> std::io::Result<()> {
    empty_dir(&home().join(".Trash"))
}

fn empty_dir(dir: &Path) -> std::io::Result<()> {
    let mut failed = 0;
    for e in std::fs::read_dir(dir)?.filter_map(Result::ok) {
        let p = e.path();
        // remove_dir_all never follows symlinks; a symlink entry is unlinked.
        let r = match e.file_type() {
            Ok(t) if t.is_dir() => std::fs::remove_dir_all(&p),
            _ => std::fs::remove_file(&p),
        };
        failed += r.is_err() as usize;
    }
    if failed == 0 { Ok(()) } else { Err(std::io::Error::other(format!("{failed} item(s) in the Trash could not be removed"))) }
}

/// Reveal in Finder. Only absolute paths (an argument starting with `-` would be read as an option),
/// and spawned without waiting so the caller's thread is never blocked.
pub fn reveal(path: &Path) {
    if path.is_absolute() {
        let _ = Command::new("/usr/bin/open").arg("-R").arg(path).stdin(std::process::Stdio::null()).spawn();
    }
}

/// Local UTC offset in seconds (east positive), for "snooze until tomorrow".
pub fn utc_offset(now: u64) -> i64 {
    let t = now as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return 0;
    }
    tm.tm_gmtoff as i64
}

// ---------- "where did my space go" ----------

use crate::engine::space::Location;

/// Well-known space hogs on macOS. Read-only; several are what Storage Settings calls "System Data".
pub fn space_locations(home: &Path) -> Vec<Location> {
    let lib = home.join("Library");
    let photos: Vec<PathBuf> = std::fs::read_dir(home.join("Pictures"))
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "photoslibrary")).collect())
        .unwrap_or_default();
    let l = |id, label, note, paths| Location { id, label, note, paths };
    vec![
        l("docker", "Docker Desktop disk image", "Grows as images and volumes pile up and does not shrink on its own. Run \"docker system prune\" yourself (it removes unused images); never delete the file.",
          vec![lib.join("Containers/com.docker.docker/Data/vms")]),
        l("simulators", "iOS Simulator devices", "Each simulator keeps its own apps and data. Remove ones for old runtimes in Tools.",
          vec![lib.join("Developer/CoreSimulator/Devices")]),
        l("xcode", "Xcode data", "DerivedData, archives and device support. Smart Clean handles the regenerable parts.",
          vec![lib.join("Developer/Xcode")]),
        l("ai-models", "Local AI models", "Hugging Face and Ollama downloads. Smart Clean lists them per model.",
          vec![home.join(".cache/huggingface"), home.join(".ollama/models")]),
        l("ios-backups", "iPhone & iPad backups", "Manage in Finder (select the device, Manage Backups).",
          vec![lib.join("Application Support/MobileSync/Backup")]),
        l("photos", "Photos library", "Turn on Optimize Mac Storage in Photos settings to keep originals only in iCloud.", photos),
        l("messages", "Messages attachments", "Delete large attachments in Messages (Settings > General > Keep messages) or in Storage Settings.",
          vec![lib.join("Messages/Attachments")]),
        l("mail", "Mail data", "Mailboxes and downloads. Mail's own settings control what is kept locally.", vec![lib.join("Mail")]),
        l("icloud", "iCloud Drive (local copies)", "Files downloaded from iCloud. \"Remove Download\" in Finder frees space and keeps them in iCloud.",
          vec![lib.join("Mobile Documents")]),
        l("caches", "App caches", "Regenerable. Smart Clean removes them safely.", vec![lib.join("Caches")]),
        l("trash", "Trash", "Empty it in Smart Clean or Finder.", vec![home.join(".Trash")]),
        l("homebrew", "Homebrew", "Installed packages plus old versions. \"brew cleanup\" removes old versions and downloads (Tools).",
          vec![PathBuf::from("/opt/homebrew"), PathBuf::from("/usr/local/Cellar")]),
    ]
}

// ---------- maintenance tools (official CLIs only) ----------

fn run(cmd: &str, args: &[&str]) -> Option<std::process::Output> {
    Command::new(cmd).args(args).stdin(std::process::Stdio::null()).output().ok()
}

/// `tmutil listlocalsnapshots /` lines look like `com.apple.TimeMachine.2026-11-26-101010.local`.
pub fn parse_tm_snapshots(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| l.trim().strip_prefix("com.apple.TimeMachine.")?.strip_suffix(".local").map(str::to_string))
        .filter(|d| valid_snapshot_date(d))
        .collect()
}

/// Strict `YYYY-MM-DD-HHMMSS`: the only thing we ever pass to tmutil.
pub fn valid_snapshot_date(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 17
        && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 || i == 10 { *c == b'-' } else { c.is_ascii_digit() })
}

pub fn tm_snapshots() -> Vec<String> {
    run("/usr/bin/tmutil", &["listlocalsnapshots", "/"]).map(|o| parse_tm_snapshots(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

pub fn tm_delete_command(date: &str) -> String {
    format!("/usr/bin/tmutil deletelocalsnapshots {date}")
}

/// Deleting local snapshots needs admin rights. We never sudo: macOS shows its own
/// administrator password dialog (osascript `with administrator privileges`), so the user sees
/// and approves the exact command. `date` is validated before it reaches the shell.
pub fn tm_delete(date: &str) -> Result<(), String> {
    if !valid_snapshot_date(date) {
        return Err("invalid snapshot date".into());
    }
    let script = format!("do shell script \"{}\" with administrator privileges", tm_delete_command(date));
    let o = run("/usr/bin/osascript", &["-e", &script]).ok_or("could not start osascript")?;
    if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
}

/// (count, bytes) of simulator devices whose runtime is gone, from `simctl list -j`.
pub fn parse_unavailable_sims(json: &str) -> (usize, u64) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return (0, 0) };
    let Some(devs) = v.get("devices").and_then(|d| d.as_object()) else { return (0, 0) };
    devs.values()
        .filter_map(|a| a.as_array())
        .flatten()
        .filter(|d| d.get("isAvailable").and_then(|x| x.as_bool()) == Some(false))
        .fold((0, 0), |(n, b), d| (n + 1, b + d.get("dataPathSize").and_then(|x| x.as_u64()).unwrap_or(0)))
}

/// Only call simctl when a full Xcode is selected; a bare `xcrun` would pop the CLT installer.
fn has_simctl() -> bool {
    run("/usr/bin/xcode-select", &["-p"]).is_some_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).contains("Xcode"))
}

pub fn unavailable_sims() -> Option<(usize, u64)> {
    if !has_simctl() {
        return None;
    }
    run("/usr/bin/xcrun", &["simctl", "list", "devices", "unavailable", "-j"]).map(|o| parse_unavailable_sims(&String::from_utf8_lossy(&o.stdout)))
}

pub const SIMS_DELETE_COMMAND: &str = "xcrun simctl delete unavailable";

pub fn delete_unavailable_sims() -> Result<(), String> {
    let o = run("/usr/bin/xcrun", &["simctl", "delete", "unavailable"]).ok_or("could not start xcrun")?;
    if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
}

pub fn brew_path() -> Option<&'static str> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].into_iter().find(|p| Path::new(p).exists())
}

/// From `brew cleanup --dry-run`: (number of paths it would remove, brew's own size estimate).
pub fn parse_brew_dry_run(out: &str) -> (usize, Option<String>) {
    let n = out.lines().filter(|l| l.starts_with("Would remove:")).count();
    let est = out.lines().find_map(|l| {
        let rest = l.split("would free approximately ").nth(1)?;
        Some(rest.split(" of disk space").next()?.trim().to_string())
    });
    (n, est)
}

pub fn brew_preview() -> Option<(usize, Option<String>)> {
    let b = brew_path()?;
    let o = Command::new(b).args(["cleanup", "--dry-run"]).env("HOMEBREW_NO_AUTO_UPDATE", "1").stdin(std::process::Stdio::null()).output().ok()?;
    let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    Some(parse_brew_dry_run(&text))
}

pub const BREW_CLEANUP_COMMAND: &str = "brew cleanup";

pub fn brew_cleanup() -> Result<(), String> {
    let b = brew_path().ok_or("Homebrew not found")?;
    let o = Command::new(b).arg("cleanup").env("HOMEBREW_NO_AUTO_UPDATE", "1").stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
    if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tm_snapshots_and_rejects_injection() {
        let out = "Snapshots for disk /:\ncom.apple.TimeMachine.2026-11-26-101010.local\ncom.apple.TimeMachine.2026-11-25-090000.local\ncom.apple.TimeMachine.2026-11-25-0900; rm -rf ~.local\nother\n";
        assert_eq!(parse_tm_snapshots(out), vec!["2026-11-26-101010", "2026-11-25-090000"]);
        assert!(valid_snapshot_date("2026-11-26-101010"));
        for bad in ["2026-11-26-10101", "2026-11-26-1010100", "2026-11-26 101010", "x026-11-26-101010", "\"; rm -rf ~"] {
            assert!(!valid_snapshot_date(bad), "{bad}");
        }
        assert!(tm_delete("$(id)").is_err());
    }

    #[test]
    fn trash_never_overwrites_and_keeps_extension() {
        let d = std::env::temp_dir().join(format!("tidy-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (src, bin) = (d.join("src"), d.join("bin"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("a.dmg"), b"old").unwrap();
        std::fs::write(src.join("a.dmg"), b"new").unwrap();
        trash_into(&src.join("a.dmg"), &bin).unwrap();
        assert_eq!(std::fs::read(bin.join("a.dmg")).unwrap(), b"old", "existing item untouched");
        assert_eq!(std::fs::read(bin.join("a 2.dmg")).unwrap(), b"new");
        std::fs::create_dir_all(src.join("folder")).unwrap();
        trash_into(&src.join("folder"), &bin).unwrap();
        assert!(bin.join("folder").is_dir() && !src.join("folder").exists());
        // empty: everything goes, symlinks are unlinked without touching their target
        let keep = d.join("keep");
        std::fs::write(&keep, b"k").unwrap();
        std::os::unix::fs::symlink(&keep, bin.join("link")).unwrap();
        empty_dir(&bin).unwrap();
        assert_eq!(std::fs::read_dir(&bin).unwrap().count(), 0);
        assert!(keep.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn parses_simctl_unavailable() {
        let j = r#"{"devices":{"rt1":[{"isAvailable":false,"dataPathSize":100},{"isAvailable":true,"dataPathSize":5}],"rt2":[{"isAvailable":false}]}}"#;
        assert_eq!(parse_unavailable_sims(j), (2, 100));
        assert_eq!(parse_unavailable_sims("not json"), (0, 0));
    }

    #[test]
    fn parses_brew_dry_run() {
        let out = "Would remove: /Users/x/Library/Caches/Homebrew/foo--1.0 (5MB)\nWould remove: /opt/homebrew/Cellar/bar/1.0 (12 files, 3MB)\n==> This operation would free approximately 8MB of disk space.\n";
        assert_eq!(parse_brew_dry_run(out), (2, Some("8MB".into())));
        assert_eq!(parse_brew_dry_run(""), (0, None));
    }
}
