//! Executes a cleaning plan. Every path is authorised by `guard` immediately before it is touched.

use super::{catalog::Disposal, guard};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub struct Job {
    pub path: PathBuf,
    pub size: u64,
    pub disposal: Disposal,
}

#[derive(Serialize, Default, Debug)]
pub struct Report {
    pub freed: u64,
    pub removed: usize,
    pub failed: Vec<Failure>,
    /// Successfully handled items, for the audit log (not sent to the UI).
    #[serde(skip)]
    pub done: Vec<(PathBuf, u64, Disposal)>,
}

#[derive(Serialize, Debug)]
pub struct Failure {
    pub path: String,
    pub reason: String,
}

pub fn run(jobs: Vec<Job>, home: &Path, empty_trash: impl Fn() -> std::io::Result<()>, trash: impl Fn(&Path) -> std::io::Result<()>) -> Report {
    let mut r = Report::default();
    for j in jobs {
        let res: Result<(), String> = match j.disposal {
            Disposal::EmptyTrash => empty_trash().map_err(|e| e.to_string()),
            Disposal::Delete | Disposal::Trash => match guard::authorize(&j.path, home) {
                Err(d) => Err(format!("blocked by safety guard: {d:?}")),
                Ok(p) => match j.disposal {
                    Disposal::Trash => trash(&p).map_err(|e| e.to_string()),
                    _ => remove(&p).map_err(|e| e.to_string()),
                },
            },
        };
        match res {
            Ok(()) => {
                r.freed += j.size;
                r.removed += 1;
                r.done.push((j.path, j.size, j.disposal));
            }
            Err(reason) => r.failed.push(Failure { path: j.path.display().to_string(), reason }),
        }
    }
    r
}

fn remove(p: &Path) -> std::io::Result<()> {
    let md = std::fs::symlink_metadata(p)?;
    if md.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn deletes_allowed_and_refuses_protected() {
        let h = std::env::temp_dir().join(format!("tidy-clean-{}", std::process::id()));
        let _ = fs::remove_dir_all(&h);
        fs::create_dir_all(h.join("Library/Caches/x")).unwrap();
        fs::write(h.join("Library/Caches/x/f"), b"hi").unwrap();
        fs::create_dir_all(h.join("Documents")).unwrap();
        let jobs = vec![
            Job { path: h.join("Library/Caches/x"), size: 10, disposal: Disposal::Delete },
            Job { path: h.join("Documents"), size: 99, disposal: Disposal::Delete },
        ];
        let r = run(jobs, &h, || Ok(()), |_| Ok(()));
        assert_eq!((r.removed, r.freed, r.failed.len(), r.done.len()), (1, 10, 1, 1));
        assert!(!h.join("Library/Caches/x").exists() && h.join("Documents").exists());
    }
}
