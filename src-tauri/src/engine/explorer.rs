//! Read-only storage explorer: drill into any readable folder, see what is taking space.
//! Never crosses filesystem boundaries (no double counting of /Volumes, firmlinks) and never follows symlinks.

use rayon::prelude::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use parking_lot::Mutex;
use walkdir::WalkDir;

#[derive(Serialize, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
    /// Some content under this entry could not be read (permissions).
    pub blocked: bool,
    pub modified: Option<u64>,
    /// Size change since the baseline snapshot (filled in by the caller from the store).
    pub delta: Option<i64>,
}

/// At most this many entries are returned (largest first); the rest are summarised in `hidden`.
/// Keeps IPC payloads and the UI bounded for folders with tens of thousands of children.
pub const MAX_ENTRIES: usize = 2000;

#[derive(Serialize, Debug)]
pub struct Listing {
    pub path: PathBuf,
    pub total: u64,
    pub entries: Vec<Entry>,
    /// Number of smaller entries left out of `entries` (still counted in `total`).
    pub hidden: usize,
    pub unreadable: bool,
    /// When the baseline used for `Entry::delta` was taken (unix secs), if any.
    pub since: Option<u64>,
}

type Seen = Mutex<HashSet<(u64, u64)>>;

fn secs(t: std::io::Result<std::time::SystemTime>) -> Option<u64> {
    t.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs())
}

/// Sum allocated bytes under `p`, staying on device `dev`, counting each hardlinked inode once.
fn tally(p: &Path, dev: u64, seen: &Seen, cancel: &AtomicBool) -> (u64, bool) {
    let (mut size, mut blocked) = (0, false);
    for e in WalkDir::new(p).follow_links(false) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(e) = e else { blocked = true; continue };
        let Ok(m) = e.metadata() else { blocked = true; continue };
        if m.dev() != dev || m.is_dir() {
            continue;
        }
        if m.nlink() > 1 && !seen.lock().insert((m.dev(), m.ino())) {
            continue;
        }
        size += m.blocks() * 512;
    }
    (size, blocked)
}

/// Fill `delta` for each entry from a baseline of child sizes. New children count fully as growth;
/// unchanged sizes stay `None` so the UI only highlights real change.
pub fn apply_baseline(l: &mut Listing, ts: u64, base: &HashMap<String, u64>) {
    l.since = Some(ts);
    for e in &mut l.entries {
        let before = base.get(&e.name).copied().unwrap_or(0);
        let d = e.size as i64 - before as i64;
        e.delta = (d != 0).then_some(d);
    }
}

/// Device id of `p` (0 if it can't be read).
pub fn dev_of(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.dev()).unwrap_or(0)
}

/// List `dir`'s children with their sizes. Children on another device are skipped (mounted volumes),
/// except devices in `also`: on macOS, `/Users`, `/Applications`... under `/` are firmlinks into the Data
/// volume, so listing `/` passes the Data volume's device here. Each child is sized on its own device,
/// so the Data volume reachable again through `/System/Volumes/Data` is never counted twice.
pub fn list(dir: &Path, also: &[u64], cancel: &AtomicBool) -> Listing {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Listing { path: dir.to_path_buf(), total: 0, entries: vec![], hidden: 0, unreadable: true, since: None };
    };
    let dev = dev_of(dir);
    let seen: Seen = Mutex::new(HashSet::new());
    let children: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
    let mut entries: Vec<Entry> = children
        .par_iter()
        .filter_map(|p| {
            let m = std::fs::symlink_metadata(p).ok()?;
            if m.file_type().is_symlink() || (m.dev() != dev && !also.contains(&m.dev())) {
                return None;
            }
            let (size, blocked) = if m.is_dir() { tally(p, m.dev(), &seen, cancel) } else { (m.blocks() * 512, false) };
            Some(Entry { name: p.file_name()?.to_string_lossy().into_owned(), path: p.clone(), size, is_dir: m.is_dir(), blocked, modified: secs(m.modified()), delta: None })
        })
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.size));
    let total = entries.iter().map(|e| e.size).sum();
    let hidden = entries.len().saturating_sub(MAX_ENTRIES);
    entries.truncate(MAX_ENTRIES);
    Listing { path: dir.to_path_buf(), total, entries, hidden, unreadable: false, since: None }
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Video,
    Images,
    Audio,
    Documents,
    Archives,
    Code,
    Apps,
    Other,
}

pub fn kind_of(p: &Path) -> Kind {
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "mp4" | "mov" | "mkv" | "avi" | "m4v" | "webm" | "wmv" => Kind::Video,
        "jpg" | "jpeg" | "png" | "heic" | "gif" | "tiff" | "raw" | "dng" | "psd" | "webp" | "svg" => Kind::Images,
        "mp3" | "m4a" | "wav" | "flac" | "aiff" | "aac" => Kind::Audio,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "pages" | "numbers" | "key" | "txt" | "md" | "csv" => Kind::Documents,
        "zip" | "gz" | "tar" | "tgz" | "7z" | "rar" | "dmg" | "pkg" | "iso" | "xz" | "bz2" => Kind::Archives,
        "rs" | "js" | "ts" | "tsx" | "py" | "go" | "java" | "c" | "cpp" | "h" | "swift" | "rb" | "php" | "json" | "o" | "a" | "dylib" => Kind::Code,
        "app" | "framework" | "ipa" => Kind::Apps,
        _ => Kind::Other,
    }
}

#[derive(Serialize, Debug)]
pub struct Insights {
    pub path: PathBuf,
    pub total: u64,
    pub by_kind: Vec<(Kind, u64)>,
    pub top_files: Vec<Entry>,
    pub unreadable_dirs: u64,
}

struct Part {
    kinds: HashMap<Kind, u64>,
    files: Vec<(u64, PathBuf, Option<u64>)>,
    unreadable: u64,
}

/// One pass over `root`: bytes per file kind plus the N largest files.
/// `also`: extra devices allowed for direct children, as in [`list`].
pub fn insights(root: &Path, top_n: usize, also: &[u64], cancel: &AtomicBool) -> Insights {
    let root_dev = dev_of(root);
    let seen: Seen = Mutex::new(HashSet::new());
    let tops: Vec<PathBuf> = std::fs::read_dir(root).map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect()).unwrap_or_default();
    let parts: Vec<Part> = tops
        .par_iter()
        .map(|top| {
            let mut part = Part { kinds: HashMap::new(), files: Vec::new(), unreadable: 0 };
            let top_dev = std::fs::symlink_metadata(top).map(|m| m.dev()).unwrap_or(root_dev);
            let dev = if also.contains(&top_dev) { top_dev } else { root_dev };
            for e in WalkDir::new(top).follow_links(false) {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(e) = e else { part.unreadable += 1; continue };
                let Ok(m) = e.metadata() else { part.unreadable += 1; continue };
                if m.dev() != dev || !m.is_file() {
                    continue;
                }
                if m.nlink() > 1 && !seen.lock().insert((m.dev(), m.ino())) {
                    continue;
                }
                let size = m.blocks() * 512;
                let in_bundle = e.path().ancestors().any(|a| a.extension().is_some_and(|x| x == "app" || x == "framework"));
                let kind = if in_bundle { Kind::Apps } else { kind_of(e.path()) };
                *part.kinds.entry(kind).or_default() += size;
                part.files.push((size, e.into_path(), secs(m.modified())));
                if part.files.len() > top_n * 4 {
                    part.files.sort_by_key(|f| std::cmp::Reverse(f.0));
                    part.files.truncate(top_n);
                }
            }
            part
        })
        .collect();
    let mut kinds: HashMap<Kind, u64> = HashMap::new();
    let (mut files, mut unreadable_dirs) = (Vec::new(), 0);
    for p in parts {
        for (k, v) in p.kinds {
            *kinds.entry(k).or_default() += v;
        }
        files.extend(p.files);
        unreadable_dirs += p.unreadable;
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.truncate(top_n);
    let mut by_kind: Vec<_> = kinds.into_iter().collect();
    by_kind.sort_by_key(|k| std::cmp::Reverse(k.1));
    Insights {
        path: root.to_path_buf(),
        total: by_kind.iter().map(|k| k.1).sum(),
        by_kind,
        top_files: files
            .into_iter()
            .map(|(size, path, modified)| Entry { name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), path, size, is_dir: false, blocked: false, modified, delta: None })
            .collect(),
        unreadable_dirs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn lists_sorted_and_counts_hardlinks_once() {
        let d = std::env::temp_dir().join(format!("tidy-exp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("big")).unwrap();
        fs::create_dir_all(d.join("small")).unwrap();
        fs::write(d.join("big/a.mp4"), vec![1u8; 400_000]).unwrap();
        fs::hard_link(d.join("big/a.mp4"), d.join("big/b.mp4")).unwrap();
        fs::write(d.join("small/x.txt"), vec![1u8; 10_000]).unwrap();
        let l = list(&d, &[], &AtomicBool::new(false));
        assert_eq!(l.entries[0].name, "big");
        assert!(l.entries[0].size < 600_000, "hardlink double counted: {}", l.entries[0].size);
        let i = insights(&d, 5, &[], &AtomicBool::new(false));
        assert_eq!(i.by_kind[0].0, Kind::Video);
        assert!(i.top_files[0].name.ends_with(".mp4"));
        let mut l = l;
        let base = HashMap::from([("big".to_string(), l.entries[0].size), ("small".to_string(), 1)]);
        apply_baseline(&mut l, 7, &base);
        assert_eq!(l.since, Some(7));
        assert_eq!(l.entries[0].delta, None, "unchanged");
        assert!(l.entries[1].delta.unwrap() > 0, "grew");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn caps_entries_but_keeps_the_total() {
        let d = std::env::temp_dir().join(format!("tidy-exp-cap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        for i in 0..MAX_ENTRIES + 7 {
            fs::write(d.join(format!("f{i}")), b"x").unwrap();
        }
        let l = list(&d, &[], &AtomicBool::new(false));
        assert_eq!((l.entries.len(), l.hidden), (MAX_ENTRIES, 7));
        assert!(l.total >= l.entries.iter().map(|e| e.size).sum::<u64>());
        let _ = fs::remove_dir_all(&d);
    }
}
