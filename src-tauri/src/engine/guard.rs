//! The single authority on whether a path may be destroyed.
//! Every destructive call in `cleaner` goes through `authorize`.

use std::path::{Path, PathBuf};

/// Top-level folders in $HOME that are never deletable as a whole.
const PROTECTED_HOME_DIRS: &[&str] = &[
    "Desktop", "Documents", "Downloads", "Library", "Movies", "Music", "Pictures", "Public",
    "Applications", ".ssh", ".gnupg", ".aws", ".kube",
];

/// Subtrees of $HOME where nothing may be removed at any depth: credentials, synced cloud data,
/// and databases whose internals must only be changed by their owning app.
const PROTECTED_SUBTREES: &[&str] = &[
    ".ssh", ".gnupg", ".aws", ".kube", ".docker", ".config/gcloud",
    "Library/Keychains", "Library/Mobile Documents", "Library/CloudStorage", "Library/Accounts",
    "Library/Messages", "Library/Mail", "Library/Safari", "Library/Calendars", "Library/Application Support/AddressBook",
    "Library/Group Containers/group.com.apple.notes",
];

#[derive(Debug, PartialEq, Eq)]
pub enum Denied {
    NotAbsolute,
    OutsideHome,
    TooShallow,
    Protected,
    Missing,
    /// Inside an app-managed bundle/library (Photos library, app bundle) or a git work tree root.
    ManagedData,
}

/// Case-insensitive, component-wise "starts with". The default APFS volume is case-insensitive, so
/// `~/library/keychains` names the same folder as `~/Library/Keychains` and must be protected too.
/// (APFS is also normalisation-insensitive; every protected name is ASCII, so Unicode forms cannot alias it.)
fn starts_with_ci(rel: &[String], prefix: &str) -> bool {
    let want: Vec<String> = prefix.split('/').map(str::to_lowercase).collect();
    rel.len() >= want.len() && rel.iter().zip(&want).all(|(a, b)| a == b)
}

/// Returns the path to operate on if deletion is allowed.
/// Parent is canonicalised (so `..` and symlinked parents can't escape $HOME);
/// the final component is NOT followed, so a symlink is unlinked, never its target.
/// Callers must call this immediately before the destructive syscall (TOCTOU window kept minimal).
pub fn authorize(path: &Path, home: &Path) -> Result<PathBuf, Denied> {
    if !path.is_absolute() {
        return Err(Denied::NotAbsolute);
    }
    // `file_name()` is None for paths ending in `..`; a trailing `/` or `/.` is normalised away by Path.
    let name = path.file_name().ok_or(Denied::TooShallow)?;
    if name == "." || name == ".." {
        return Err(Denied::TooShallow);
    }
    let parent = path.parent().ok_or(Denied::TooShallow)?;
    let parent = parent.canonicalize().map_err(|_| Denied::Missing)?;
    let home = home.canonicalize().map_err(|_| Denied::Missing)?;
    let full = parent.join(name);
    let Ok(md) = std::fs::symlink_metadata(&full) else { return Err(Denied::Missing) };
    let rel = full.strip_prefix(&home).map_err(|_| Denied::OutsideHome)?;
    // Lower-cased components for every name comparison below (case-insensitive APFS).
    let comps: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    if comps.len() < 2 {
        // $HOME itself or a direct child like ~/Documents
        return Err(if comps.len() == 1 && is_protected(&comps[0]) { Denied::Protected } else { Denied::TooShallow });
    }
    // ~/Library/<X> as a whole (Caches, Preferences, Group Containers, Safari, ...) is never removable:
    // the catalog only ever targets children *inside* these folders.
    if comps.len() == 2 && comps[0] == "library" {
        return Err(Denied::Protected);
    }
    if PROTECTED_SUBTREES.iter().any(|p| starts_with_ci(&comps, p)) {
        return Err(Denied::Protected);
    }
    // Never reach *inside* an app-managed library package: only its app may change its contents.
    let inside_library = comps[..comps.len() - 1].iter().any(|s| MANAGED_PACKAGES.iter().any(|ext| s.ends_with(ext)));
    if inside_library {
        return Err(Denied::ManagedData);
    }
    // A directory that is itself a git work tree is source code, never "build output" or cache.
    if md.is_dir() && full.join(".git").symlink_metadata().is_ok() {
        return Err(Denied::ManagedData);
    }
    Ok(full)
}

/// Package extensions whose contents belong to one app (Photos, Music, TV, iMovie, Final Cut, Logic).
pub const MANAGED_PACKAGES: &[&str] = &[".photoslibrary", ".photolibrary", ".musiclibrary", ".tvlibrary", ".imovielibrary", ".fcpbundle", ".logicx"];

fn is_protected(c: &str) -> bool {
    PROTECTED_HOME_DIRS.iter().any(|p| p.eq_ignore_ascii_case(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_home(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tidy-guard-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("Library/Caches/app")).unwrap();
        fs::create_dir_all(d.join("Documents")).unwrap();
        d
    }

    #[test]
    fn allows_cache_child() {
        let h = tmp_home("a");
        assert!(authorize(&h.join("Library/Caches/app"), &h).is_ok());
    }
    #[test]
    fn blocks_home_and_protected_roots() {
        let h = tmp_home("b");
        assert_eq!(authorize(&h, &h), Err(Denied::TooShallow));
        assert_eq!(authorize(&h.join("Documents"), &h), Err(Denied::Protected));
        assert_eq!(authorize(&h.join("Library"), &h), Err(Denied::Protected));
    }
    #[test]
    fn blocks_outside_home_and_dotdot() {
        let h = tmp_home("c");
        let outside = std::env::temp_dir().join(format!("tidy-out-{}", std::process::id()));
        fs::create_dir_all(outside.join("x")).unwrap();
        assert_eq!(authorize(&outside.join("x"), &h), Err(Denied::OutsideHome));
        let sneaky = h.join("Library/Caches/app/../../../../");
        assert!(authorize(&sneaky, &h).is_err());
    }
    #[test]
    fn symlink_escape_is_not_followed() {
        let h = tmp_home("d");
        let outside = std::env::temp_dir().join(format!("tidy-tgt-{}", std::process::id()));
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, h.join("Library/Caches/link")).unwrap();
        // the link itself is deletable (unlink), and resolves to the link path, not the target
        let p = authorize(&h.join("Library/Caches/link"), &h).unwrap();
        assert!(p.ends_with("Library/Caches/link"));
        // a path *through* the symlinked parent escapes home and is denied
        assert_eq!(authorize(&h.join("Library/Caches/link/inner"), &h), Err(Denied::Missing));
    }
    #[test]
    fn protected_subtrees_are_blocked_at_any_depth() {
        let h = tmp_home("e");
        for rel in [".ssh/keys/id", "Library/Keychains/x/y", "Library/Mobile Documents/com~apple~CloudDocs/f", ".docker/contexts/a", "Library/Messages/Attachments/ab"] {
            fs::create_dir_all(h.join(rel)).unwrap();
            assert_eq!(authorize(&h.join(rel), &h), Err(Denied::Protected), "{rel}");
        }
        // a lookalike name is not protected
        fs::create_dir_all(h.join(".sshfoo/x")).unwrap();
        assert!(authorize(&h.join(".sshfoo/x"), &h).is_ok());
    }
    #[test]
    fn photos_library_internals_and_git_roots_are_blocked() {
        let h = tmp_home("f");
        fs::create_dir_all(h.join("Pictures/Photos Library.photoslibrary/resources/derivatives")).unwrap();
        assert_eq!(authorize(&h.join("Pictures/Photos Library.photoslibrary/resources/derivatives"), &h), Err(Denied::ManagedData));
        fs::create_dir_all(h.join("code/app/.git")).unwrap();
        fs::create_dir_all(h.join("code/app/target")).unwrap();
        assert_eq!(authorize(&h.join("code/app"), &h), Err(Denied::ManagedData));
        assert!(authorize(&h.join("code/app/target"), &h).is_ok(), "build output inside a repo stays cleanable");
        fs::create_dir_all(h.join("Music/Music/Music Library.musiclibrary/x")).unwrap();
        assert_eq!(authorize(&h.join("Music/Music/Music Library.musiclibrary/x"), &h), Err(Denied::ManagedData));
    }
    #[test]
    fn case_variants_cannot_bypass_protection() {
        // The default APFS volume is case-insensitive: ~/library/keychains IS ~/Library/Keychains.
        let h = tmp_home("g");
        fs::create_dir_all(h.join("Library/Keychains/db")).unwrap();
        fs::create_dir_all(h.join(".SSH/k")).unwrap();
        assert_eq!(authorize(&h.join("library/keychains/db"), &h), Err(Denied::Protected));
        assert_eq!(authorize(&h.join("LIBRARY/Keychains/db"), &h), Err(Denied::Protected));
        assert_eq!(authorize(&h.join(".SSH/k"), &h), Err(Denied::Protected));
        assert_eq!(authorize(&h.join("documents"), &h), Err(Denied::Protected));
    }
    #[test]
    fn library_children_and_dot_paths_are_never_whole_targets() {
        let h = tmp_home("h");
        for d in ["Library/Caches", "Library/Group Containers", "Library/Preferences"] {
            fs::create_dir_all(h.join(d)).unwrap();
            assert_eq!(authorize(&h.join(d), &h), Err(Denied::Protected), "{d}");
        }
        assert!(authorize(&h.join("Library/Caches/app/."), &h).is_ok_and(|p| p.ends_with("Library/Caches/app")));
        assert!(authorize(&h.join("Library/Caches/app/.."), &h).is_err(), "`..` final component");
        assert!(authorize(&h.join("Library/Caches/app/"), &h).is_ok(), "trailing slash is the same path");
        assert_eq!(authorize(Path::new("Library/Caches/app"), &h), Err(Denied::NotAbsolute));
        assert_eq!(authorize(&h.join("Library/Caches/missing"), &h), Err(Denied::Missing));
    }
}
