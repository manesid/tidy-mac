//! Parallel discovery + sizing. Read-only; never mutates the filesystem.

use super::catalog::{ArtifactRule, Category, Source};
use rayon::prelude::*;
use serde::Serialize;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

#[derive(Serialize, Clone, Debug)]
pub struct Found {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub items: Vec<Found>,
    /// Set when the source exists but couldn't be read (typically missing Full Disk Access).
    pub blocked: bool,
}

/// Physical size on disk (allocated blocks), not following symlinks.
pub fn size_of(path: &Path, cancel: &AtomicBool) -> u64 {
    let Ok(md) = std::fs::symlink_metadata(path) else { return 0 };
    if !md.is_dir() {
        return md.blocks() * 512;
    }
    let mut total = 0;
    for e in WalkDir::new(path).follow_links(false).into_iter().filter_map(Result::ok) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if let Ok(m) = e.metadata() {
            if !m.is_dir() {
                total += m.blocks() * 512;
            }
        }
    }
    total
}

fn found(path: PathBuf, cancel: &AtomicBool) -> Found {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let size = size_of(&path, cancel);
    Found { path, name, size }
}

fn sized(paths: Vec<PathBuf>, cancel: &AtomicBool) -> Vec<Found> {
    let mut v: Vec<Found> = paths.into_par_iter().map(|p| found(p, cancel)).filter(|f| f.size > 0).collect();
    v.sort_by_key(|f| std::cmp::Reverse(f.size));
    v
}

pub fn scan(cat: &Category, home: &Path, cancel: &AtomicBool) -> Outcome {
    scan_source(&cat.source, home, cancel)
}

fn scan_source(source: &Source, home: &Path, cancel: &AtomicBool) -> Outcome {
    match source {
        Source::Children { dir, skip } => match std::fs::read_dir(dir) {
            Ok(rd) => {
                let paths = rd
                    .filter_map(Result::ok)
                    .filter(|e| { let n = e.file_name(); let n = n.to_string_lossy(); !skip.iter().any(|p| n.starts_with(p)) })
                    .map(|e| e.path())
                    .collect();
                Outcome { items: sized(paths, cancel), blocked: false }
            }
            Err(e) => Outcome { items: vec![], blocked: e.kind() == std::io::ErrorKind::PermissionDenied },
        },
        Source::Fixed(paths) => Outcome { items: sized(paths.iter().filter(|p| p.exists()).cloned().collect(), cancel), blocked: false },
        Source::Files { dir, exts } => {
            let paths = std::fs::read_dir(dir).map(|rd| {
                rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| exts.iter().any(|e| x.eq_ignore_ascii_case(e)))).collect()
            }).unwrap_or_default();
            Outcome { items: sized(paths, cancel), blocked: false }
        }
        Source::TrashBin => {
            let p = home.join(".Trash");
            match std::fs::read_dir(&p) {
                Ok(_) => {
                    let size = size_of(&p, cancel);
                    Outcome { items: if size > 0 { vec![Found { path: p, name: "Trash".into(), size }] } else { vec![] }, blocked: false }
                }
                Err(e) => Outcome { items: vec![], blocked: e.kind() == std::io::ErrorKind::PermissionDenied },
            }
        }
        Source::LargeFiles { min_bytes, max_items } => Outcome { items: large_files(home, *min_bytes, *max_items, cancel), blocked: false },
        Source::StaleArtifacts { rules, older_than_days } => Outcome { items: stale_artifacts(home, rules, *older_than_days, cancel), blocked: false },
        Source::Many(list) => {
            let mut out = Outcome::default();
            for s in list {
                let o = scan_source(s, home, cancel);
                out.items.extend(o.items);
                out.blocked |= o.blocked;
            }
            out.items.sort_by_key(|f| std::cmp::Reverse(f.size));
            out
        }
    }
}

/// Directories we never descend into when walking $HOME for user files.
/// App-managed libraries (Photos, Music, TV...) are skipped: their files must only be changed by their
/// app, and reading them can trigger macOS privacy prompts (Photos, Media & Apple Music).
fn skip_dir(name: &str) -> bool {
    matches!(name, "Library" | ".Trash" | "node_modules" | ".git" | "Applications" | ".cargo" | ".rustup")
        || name.ends_with(".app")
        || name == "Media.localized" // Music/TV app media folder: managed by the app, privacy-protected
        || super::guard::MANAGED_PACKAGES.iter().any(|ext| name.to_ascii_lowercase().ends_with(ext))
}

fn home_children(home: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(home).map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect()).unwrap_or_default()
}

fn large_files(home: &Path, min: u64, max: usize, cancel: &AtomicBool) -> Vec<Found> {
    let mut all: Vec<Found> = home_children(home)
        .into_par_iter()
        .flat_map_iter(|top| {
            WalkDir::new(top)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| !(e.file_type().is_dir() && skip_dir(&e.file_name().to_string_lossy())))
                .filter_map(Result::ok)
                .take_while(|_| !cancel.load(Ordering::Relaxed))
                .filter(|e| e.file_type().is_file())
                .filter_map(|e| {
                    let size = e.metadata().ok()?.blocks() * 512;
                    (size >= min).then(|| Found { name: e.file_name().to_string_lossy().into_owned(), path: e.into_path(), size })
                })
                .collect::<Vec<_>>()
        })
        .collect();
    all.sort_by_key(|f| std::cmp::Reverse(f.size));
    all.truncate(max);
    all
}

/// One walk over $HOME finds every rule's directories (node_modules, target, .venv, ...).
fn stale_artifacts(home: &Path, rules: &[ArtifactRule], days: u64, cancel: &AtomicBool) -> Vec<Found> {
    let cutoff = SystemTime::now() - Duration::from_secs(days * 86_400);
    let paths: Vec<PathBuf> = home_children(home)
        .into_par_iter()
        .flat_map_iter(|top| find_artifacts(&top, rules, cutoff, cancel))
        .collect();
    let mut found = sized(paths, cancel);
    // Name items "<project>/<dir>" so a list of many node_modules is readable.
    for f in &mut found {
        if let Some(proj) = f.path.parent().and_then(|p| p.file_name()) {
            f.name = format!("{}/{}", proj.to_string_lossy(), f.name);
        }
    }
    found
}

fn find_artifacts(top: &Path, rules: &[ArtifactRule], cutoff: SystemTime, cancel: &AtomicBool) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    let mut it = WalkDir::new(top).follow_links(false).max_depth(8).into_iter();
    while let Some(next) = it.next() {
        let Ok(e) = next else { continue };
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if !e.file_type().is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy();
        if let Some(rule) = rules.iter().find(|r| r.dir == name) {
            if is_artifact(e.path(), rule) {
                it.skip_current_dir(); // never descend into dependency/build trees
                if project_is_stale(e.path().parent(), rule, cutoff) {
                    hits.push(e.into_path());
                }
                continue;
            }
        }
        if skip_dir(&name) {
            it.skip_current_dir();
        }
    }
    hits
}

fn is_artifact(dir: &Path, rule: &ArtifactRule) -> bool {
    let Some(project) = dir.parent() else { return false };
    if rule.requires.is_some_and(|f| !project.join(f).is_file()) {
        return false; // not a real project: leave it alone
    }
    if project.join(".git").exists() && dir.join(".git").exists() {
        return false; // a repo of its own, not build output
    }
    rule.inside_any.is_empty() || rule.inside_any.iter().any(|m| dir.join(m).exists())
}

/// A project is stale when its manifests/lockfiles AND the project dir itself haven't changed since `cutoff`.
fn project_is_stale(project: Option<&Path>, rule: &ArtifactRule, cutoff: SystemTime) -> bool {
    let Some(p) = project else { return false };
    rule.watch
        .iter()
        .chain(std::iter::once(&"."))
        .filter_map(|f| std::fs::metadata(p.join(f)).and_then(|m| m.modified()).ok())
        .all(|t| t < cutoff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sizes_a_tree_and_ignores_symlink_targets() {
        let d = std::env::temp_dir().join(format!("tidy-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("a")).unwrap();
        fs::write(d.join("a/f"), vec![1u8; 100_000]).unwrap();
        let big = std::env::temp_dir().join(format!("tidy-scan-big-{}", std::process::id()));
        fs::write(&big, vec![1u8; 5_000_000]).unwrap();
        std::os::unix::fs::symlink(&big, d.join("a/link")).unwrap();
        let s = size_of(&d, &AtomicBool::new(false));
        assert!((100_000..1_000_000).contains(&s), "size {s}");
    }

    fn age(p: &Path, days: u64) {
        let t = SystemTime::now() - Duration::from_secs(days * 86_400);
        fs::File::open(p).unwrap().set_modified(t).unwrap();
    }

    #[test]
    fn finds_only_stale_real_artifacts() {
        use crate::engine::catalog::ARTIFACT_RULES;
        let h = std::env::temp_dir().join(format!("tidy-stale-{}", std::process::id()));
        let _ = fs::remove_dir_all(&h);
        let mk = |rel: &str, files: &[&str]| {
            let d = h.join(rel);
            fs::create_dir_all(&d).unwrap();
            for f in files {
                fs::write(d.join(f), vec![1u8; 20_000]).unwrap();
            }
            d
        };
        // stale node project
        mk("code/old-web/node_modules/lib", &["index.js"]);
        mk("code/old-web", &["package.json"]);
        // stale rust project
        mk("code/old-rs/target/debug", &["bin"]);
        mk("code/old-rs/target", &[".rustc_info.json"]);
        mk("code/old-rs", &["Cargo.toml"]);
        // stale python venv
        mk("code/old-py/.venv/lib", &["x.py"]);
        mk("code/old-py/.venv", &["pyvenv.cfg"]);
        mk("code/old-py", &["requirements.txt"]);
        // fresh rust project: must be kept
        mk("code/new-rs/target", &[".rustc_info.json"]);
        mk("code/new-rs", &["Cargo.toml"]);
        // a "target" folder that is not cargo output (no Cargo.toml): must be kept
        mk("Documents/target", &["notes.txt"]);
        // a "venv" without pyvenv.cfg: must be kept
        mk("code/fake/venv", &["data.bin"]);
        for p in ["code/old-web", "code/old-web/package.json", "code/old-rs", "code/old-rs/Cargo.toml", "code/old-py", "code/old-py/requirements.txt", "code/fake"] {
            age(&h.join(p), 90);
        }
        let mut got: Vec<String> = stale_artifacts(&h, ARTIFACT_RULES, 60, &AtomicBool::new(false))
            .into_iter()
            .map(|f| f.name)
            .collect();
        got.sort();
        assert_eq!(got, vec!["old-py/.venv", "old-rs/target", "old-web/node_modules"]);
        let _ = fs::remove_dir_all(&h);
    }
}
