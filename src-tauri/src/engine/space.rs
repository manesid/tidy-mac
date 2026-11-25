//! "Where did my space go?" Read-only sizing of well-known space hogs that macOS lumps into
//! System Data or hides in ~/Library. Locations come from `platform` (OS-specific paths);
//! this module only measures them, in parallel, never crossing into other volumes.

use super::scanner;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

pub struct Location {
    pub id: &'static str,
    pub label: &'static str,
    /// One line on what it is and whether / how it can be reclaimed.
    pub note: &'static str,
    pub paths: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Hog {
    pub id: String,
    pub label: String,
    pub note: String,
    pub size: u64,
    /// First existing path, for "open in Storage Explorer".
    pub path: Option<String>,
    /// Exists but could not be read (typically needs Full Disk Access): size is a lower bound.
    pub blocked: bool,
}

fn blocked(p: &PathBuf) -> bool {
    p.is_dir() && std::fs::read_dir(p).is_err()
}

/// Measure every location (paths that don't exist count as 0) and return those that exist,
/// largest first.
pub fn measure(locs: Vec<Location>, cancel: &AtomicBool) -> Vec<Hog> {
    let mut out: Vec<Hog> = locs
        .into_par_iter()
        .filter_map(|l| {
            let existing: Vec<&PathBuf> = l.paths.iter().filter(|p| p.symlink_metadata().is_ok()).collect();
            if existing.is_empty() {
                return None;
            }
            let size = existing.iter().map(|p| scanner::size_of(p, cancel)).sum();
            Some(Hog {
                id: l.id.into(),
                label: l.label.into(),
                note: l.note.into(),
                size,
                path: existing.first().map(|p| p.display().to_string()),
                blocked: existing.iter().any(|p| blocked(p)),
            })
        })
        .filter(|h| h.size > 0 || h.blocked)
        .collect();
    out.sort_by_key(|f| std::cmp::Reverse(f.size));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn measures_existing_sorted_and_skips_missing() {
        let d = std::env::temp_dir().join(format!("tidy-space-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("a")).unwrap();
        fs::create_dir_all(d.join("b")).unwrap();
        fs::write(d.join("a/f"), vec![1u8; 50_000]).unwrap();
        fs::write(d.join("b/f"), vec![1u8; 500_000]).unwrap();
        let loc = |id, paths| Location { id, label: id, note: "", paths };
        let hogs = measure(
            vec![
                loc("a", vec![d.join("a")]),
                loc("b", vec![d.join("b"), d.join("missing")]),
                loc("none", vec![d.join("nope")]),
            ],
            &AtomicBool::new(false),
        );
        assert_eq!(hogs.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert!(hogs[0].size >= 500_000 && !hogs[0].blocked);
        assert!(hogs[0].path.as_deref().unwrap().ends_with("/b"));
        let _ = fs::remove_dir_all(&d);
    }
}
