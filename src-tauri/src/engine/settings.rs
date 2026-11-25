use super::menubar::TitleMode;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub alerts_enabled: bool,
    pub warn_pct: u8,
    pub critical_pct: u8,
    pub check_minutes: u32,
    pub launch_at_login: bool,
    pub lifetime_freed: u64,
    /// What the menu bar shows next to the icon.
    pub tray_title: TitleMode,
    /// Hide the Dock icon; Tidy lives only in the menu bar.
    pub menu_bar_only: bool,
    /// Alerts are muted until this unix time (0 = not snoozed). Set via the snooze command only.
    pub snoozed_until: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { alerts_enabled: true, warn_pct: 80, critical_pct: 90, check_minutes: 15, launch_at_login: false, lifetime_freed: 0, tray_title: TitleMode::Free, menu_bar_only: false, snoozed_until: 0 }
    }
}

impl Settings {
    /// Clamp to sane, internally consistent values (the UI is not trusted).
    pub fn sanitized(mut self) -> Self {
        self.warn_pct = self.warn_pct.clamp(50, 98);
        self.critical_pct = self.critical_pct.clamp(self.warn_pct + 1, 99);
        self.check_minutes = self.check_minutes.clamp(1, 1440);
        self
    }
    pub fn load(path: &Path) -> Self {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).unwrap_or_default().sanitized()
    }
    /// Atomic write: temp file, fsync, rename. A crash leaves either the old or the new file, never a torn one.
    /// Callers hold the settings lock, so two saves never share the temp file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(d) = path.parent() { std::fs::create_dir_all(d)?; }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sanitize_keeps_order() {
        let s = Settings { warn_pct: 95, critical_pct: 60, check_minutes: 0, ..Default::default() }.sanitized();
        assert!(s.critical_pct > s.warn_pct && s.check_minutes == 1);
    }
    #[test]
    fn levels_always_fit_and_stay_ordered() {
        for w in [0u8, 49, 50, 80, 97, 98, 99, 100, 255] {
            for c in [0u8, 50, 80, 98, 99, 100, 255] {
                let s = Settings { warn_pct: w, critical_pct: c, ..Default::default() }.sanitized();
                assert!((50..=98).contains(&s.warn_pct) && s.critical_pct > s.warn_pct && s.critical_pct <= 99, "{w}/{c} -> {}/{}", s.warn_pct, s.critical_pct);
            }
        }
    }
    #[test]
    fn roundtrip() {
        let p = std::env::temp_dir().join(format!("tidy-set-{}.json", std::process::id()));
        let s = Settings { warn_pct: 70, ..Default::default() };
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p), s.sanitized());
        // corrupt or partial files fall back to defaults instead of failing
        std::fs::write(&p, b"{\"warn_pct\": 7").unwrap();
        assert_eq!(Settings::load(&p), Settings::default());
        let _ = std::fs::remove_file(&p);
    }
}
