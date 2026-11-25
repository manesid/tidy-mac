//! Declarative list of what Tidy can clean. Adding a target = adding an entry here.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Safety {
    /// Regenerable by the OS/app; pre-selected.
    Safe,
    /// May be user data or slow to regenerate; never pre-selected.
    Review,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Disposal {
    Delete,
    Trash,
    EmptyTrash,
}

#[derive(Clone, Debug)]
pub enum Source {
    /// Each child of the directory is an item, except names starting with any of `skip`.
    Children { dir: PathBuf, skip: &'static [&'static str] },
    /// Each existing path is an item.
    Fixed(Vec<PathBuf>),
    /// Files in `dir` (depth 1) with these extensions.
    Files { dir: PathBuf, exts: &'static [&'static str] },
    LargeFiles { min_bytes: u64, max_items: usize },
    /// Regenerable project build output / dependency dirs whose project hasn't changed in N days.
    StaleArtifacts { rules: &'static [ArtifactRule], older_than_days: u64 },
    TrashBin,
    /// Union of several sources (e.g. one category spanning two tools).
    Many(Vec<Source>),
}

/// How to recognise a regenerable directory inside a project.
#[derive(Clone, Debug)]
pub struct ArtifactRule {
    /// Directory name, e.g. `node_modules`.
    pub dir: &'static str,
    /// File that must exist in the parent (the project root), if any.
    pub requires: Option<&'static str>,
    /// At least one of these must exist inside the directory (proves it is what we think).
    pub inside_any: &'static [&'static str],
    /// The project is stale only if all of these (that exist) plus the project dir are older than the cutoff.
    pub watch: &'static [&'static str],
}

pub const ARTIFACT_RULES: &[ArtifactRule] = &[
    ArtifactRule { dir: "node_modules", requires: Some("package.json"), inside_any: &[],
        watch: &["package.json", "package-lock.json", "yarn.lock", "pnpm-lock.yaml"] },
    ArtifactRule { dir: "target", requires: Some("Cargo.toml"), inside_any: &[".rustc_info.json", "CACHEDIR.TAG", "debug", "release"],
        watch: &["Cargo.toml", "Cargo.lock", "src"] },
    ArtifactRule { dir: ".venv", requires: None, inside_any: &["pyvenv.cfg"],
        watch: &["pyproject.toml", "requirements.txt", "setup.py", "uv.lock", "poetry.lock"] },
    ArtifactRule { dir: "venv", requires: None, inside_any: &["pyvenv.cfg"],
        watch: &["pyproject.toml", "requirements.txt", "setup.py", "uv.lock", "poetry.lock"] },
];

#[derive(Clone, Debug)]
pub struct Category {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub group: &'static str,
    pub safety: Safety,
    pub disposal: Disposal,
    pub needs_fda: bool,
    pub source: Source,
}

pub fn build(home: &Path) -> Vec<Category> {
    let lib = home.join("Library");
    let c = |id, name, description, group, safety, disposal, needs_fda, source| Category { id, name, description, group, safety, disposal, needs_fda, source };
    use Disposal::*;
    use Safety::*;
    vec![
        c("caches", "App caches", "Temporary files apps rebuild on demand (includes Homebrew, pip and CocoaPods download caches).", "System", Safe, Delete, false,
          Source::Children { dir: lib.join("Caches"), skip: &["com.apple."] }),
        c("logs", "Logs & crash reports", "Old diagnostic logs and crash reports.", "System", Safe, Delete, false,
          Source::Children { dir: lib.join("Logs"), skip: &[] }),
        // Review, not Safe: the Trash is the user's undo buffer (and where Tidy's own "To Trash" items go),
        // so emptying it is never pre-selected and never part of Quick Clean.
        c("trash", "Trash", "Everything currently in your Trash. Emptying it is permanent.", "System", Review, EmptyTrash, false, Source::TrashBin),
        c("mail", "Mail attachments", "Attachments Mail cached for offline viewing.", "System", Safe, Delete, true,
          Source::Children { dir: lib.join("Containers/com.apple.mail/Data/Library/Mail Downloads"), skip: &[] }),
        c("xcode-derived", "Xcode build data", "DerivedData: rebuilt on next compile.", "Developer", Safe, Delete, false,
          Source::Children { dir: lib.join("Developer/Xcode/DerivedData"), skip: &[] }),
        c("xcode-devsupport", "Xcode device support", "Symbols for old iOS versions; re-downloaded when a device connects.", "Developer", Safe, Delete, false,
          Source::Children { dir: lib.join("Developer/Xcode/iOS DeviceSupport"), skip: &[] }),
        c("sim-caches", "Simulator caches", "CoreSimulator caches; rebuilt when a simulator boots.", "Developer", Safe, Delete, false,
          Source::Children { dir: lib.join("Developer/CoreSimulator/Caches"), skip: &[] }),
        c("dev-caches", "Package manager caches", "npm, Yarn, pnpm, Gradle and Cargo download caches. Re-downloaded when needed.", "Developer", Safe, Delete, false,
          Source::Fixed(vec![home.join(".npm/_cacache"), home.join(".yarn/cache"), home.join(".gradle/caches"), lib.join("pnpm/store"), home.join(".cargo/registry/cache")])),
        // ~/.cache is shared by many CLI tools. AI model downloads live there too and are excluded
        // here: they are huge and slow to re-download, so they get their own Review category.
        c("cli-caches", "Command-line tool caches", "Per-tool folders in ~/.cache (XDG cache). Tools rebuild them on demand.", "Developer", Safe, Delete, false,
          Source::Children { dir: home.join(".cache"), skip: &["huggingface"] }),
        c("stale-builds", "Stale project dependencies & builds", "node_modules, Rust target/ and Python virtualenvs of projects untouched for 60+ days. Rebuilt by npm install, cargo build or pip install.", "Developer", Review, Delete, false,
          Source::StaleArtifacts { rules: ARTIFACT_RULES, older_than_days: 60 }),
        c("xcode-archives", "Xcode archives", "App archives with debug symbols for symbolicating crash reports. Keep the ones for versions you ship.", "Developer", Review, Trash, false,
          Source::Children { dir: lib.join("Developer/Xcode/Archives"), skip: &[] }),
        c("ai-models", "Local AI models", "Hugging Face and Ollama model downloads. Re-downloaded on next use, which can take a long time.", "AI", Review, Delete, false,
          Source::Many(vec![
              Source::Children { dir: home.join(".cache/huggingface/hub"), skip: &[".", "version"] },
              Source::Fixed(vec![home.join(".ollama/models")]),
          ])),
        c("installers", "Old installers", "Disk images and packages in Downloads.", "Files", Review, Trash, false,
          Source::Files { dir: home.join("Downloads"), exts: &["dmg", "pkg", "iso"] }),
        c("ios-backups", "iPhone & iPad backups", "Local device backups. Irreplaceable if the device is lost.", "Files", Review, Trash, true,
          Source::Children { dir: lib.join("Application Support/MobileSync/Backup"), skip: &[] }),
        c("large-files", "Large files", "Files over 500 MB in your home folder.", "Files", Review, Trash, false,
          Source::LargeFiles { min_bytes: 500 * 1024 * 1024, max_items: 50 }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_unique_and_model_caches_never_preselected() {
        let cats = build(Path::new("/Users/x"));
        let mut ids: Vec<_> = cats.iter().map(|c| c.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), cats.len());
        // Anything that is slow to regenerate or user-authored must be Review.
        for id in ["trash", "ai-models", "stale-builds", "xcode-archives", "installers", "ios-backups", "large-files"] {
            assert_eq!(cats.iter().find(|c| c.id == id).unwrap().safety, Safety::Review, "{id}");
        }
        // The Safe ~/.cache category must not swallow Hugging Face models.
        let cli = cats.iter().find(|c| c.id == "cli-caches").unwrap();
        assert!(matches!(&cli.source, Source::Children { skip, .. } if skip.contains(&"huggingface")));
        // Every Safe category deletes (regenerable data); none empties the Trash or targets user files.
        for c in cats.iter().filter(|c| c.safety == Safety::Safe) {
            assert_eq!(c.disposal, Disposal::Delete, "{}", c.id);
        }
        // PRIVACY: Safe categories feed one-click Quick Clean in the menu bar. They must never walk $HOME or
        // touch folders macOS guards with consent prompts (Desktop, Documents, Downloads, Music, Pictures, Movies, iCloud).
        let home = Path::new("/Users/x");
        let prompting = ["Desktop", "Documents", "Downloads", "Music", "Pictures", "Movies", "Library/Mobile Documents"].map(|d| home.join(d));
        fn paths(src: &Source, out: &mut Vec<PathBuf>) -> bool {
            match src {
                Source::Children { dir, .. } | Source::Files { dir, .. } => { out.push(dir.clone()); true }
                Source::Fixed(v) => { out.extend(v.iter().cloned()); true }
                Source::TrashBin => true,
                Source::LargeFiles { .. } | Source::StaleArtifacts { .. } => false, // walks all of $HOME
                Source::Many(v) => v.iter().all(|s| paths(s, out)),
            }
        }
        for c in cats.iter().filter(|c| c.safety == Safety::Safe) {
            let mut v = Vec::new();
            assert!(paths(&c.source, &mut v), "{} walks $HOME", c.id);
            assert!(!v.iter().any(|p| prompting.iter().any(|q| p.starts_with(q))), "{} touches a consent-gated folder", c.id);
        }
        // and no Safe category lists ~/.cache wholesale
        for c in cats.iter().filter(|c| c.safety == Safety::Safe) {
            if let Source::Fixed(v) = &c.source {
                assert!(!v.iter().any(|p| p.ends_with(".cache")), "{}", c.id);
            }
        }
    }
}
