//! Pure menu-bar logic: tray title text, alert snooze windows, and the state badge drawn on the
//! template tray icon. No Tauri types here so all of it is unit-testable.

use super::alerts::Level;
use serde::{Deserialize, Serialize};

/// What the menu bar shows next to the icon.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TitleMode {
    #[default]
    Free,
    Percent,
    Icon,
}

/// Decimal units, matching Finder and "About This Mac".
pub fn fmt_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let (mut v, mut i) = (b as f64, 0);
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i >= 3 { format!("{v:.1} {}", U[i]) } else { format!("{v:.0} {}", U[i]) }
}

/// Text next to the tray icon. `None` = icon only. Leading space separates it from the icon.
pub fn tray_title(mode: TitleMode, free: Option<u64>, used_pct: Option<f32>) -> Option<String> {
    match mode {
        TitleMode::Free => free.map(|f| format!(" {}", fmt_bytes(f))),
        TitleMode::Percent => used_pct.map(|p| format!(" {:.0}%", p.clamp(0.0, 100.0))),
        TitleMode::Icon => None,
    }
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Snooze {
    Hour,
    Tomorrow,
}

/// Local hour at which "until tomorrow" ends.
pub const TOMORROW_HOUR: i64 = 8;

/// Unix time at which a snooze chosen at `now` ends. `utc_offset` is the local offset in seconds
/// (east positive). "Tomorrow" means 08:00 local on the next calendar day (16-32 h away).
pub fn snooze_until(s: Snooze, now: u64, utc_offset: i64) -> u64 {
    match s {
        Snooze::Hour => now + 3600,
        Snooze::Tomorrow => {
            let local = now as i64 + utc_offset;
            let day_start = local.div_euclid(86_400) * 86_400;
            let target = day_start + 86_400 + TOMORROW_HOUR * 3600;
            (target - utc_offset) as u64
        }
    }
}

pub fn is_snoozed(until: u64, now: u64) -> bool {
    now < until
}

/// State shown on the tray icon. Template images can't carry color (macOS tints them to match the
/// menu bar), so state is shape-coded: a hollow ring for "getting full", a solid dot for critical.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Badge {
    None,
    Ring,
    Dot,
}

pub fn badge_for(level: Level) -> Badge {
    match level {
        Level::Ok => Badge::None,
        Level::Warn => Badge::Ring,
        Level::Critical => Badge::Dot,
    }
}

/// Draw `badge` in the bottom-right corner of an RGBA template image, with a transparent gap
/// around it so it reads as separate from the glyph. Only alpha matters for template images.
pub fn draw_badge(rgba: &mut [u8], w: u32, h: u32, badge: Badge) {
    if badge == Badge::None || rgba.len() < (w * h * 4) as usize {
        return;
    }
    let s = w.min(h) as f32;
    let r = s * 0.2; // badge radius
    let (cx, cy) = (w as f32 - r - 0.5, h as f32 - r - 0.5);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let i = ((y * w + x) * 4) as usize;
            let alpha = if d > r + s * 0.08 {
                continue; // outside the badge + gap: untouched glyph
            } else if d > r || (badge == Badge::Ring && d < r - s * 0.08) {
                0 // gap around the badge, or the ring's hollow centre
            } else {
                255
            };
            rgba[i..i + 4].copy_from_slice(&[0, 0, 0, alpha]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(tray_title(TitleMode::Free, Some(123_400_000_000), Some(80.0)).as_deref(), Some(" 123.4 GB"));
        assert_eq!(tray_title(TitleMode::Percent, Some(1), Some(79.6)).as_deref(), Some(" 80%"));
        assert_eq!(tray_title(TitleMode::Icon, Some(1), Some(1.0)), None);
        assert_eq!(tray_title(TitleMode::Free, None, None), None);
        assert_eq!(fmt_bytes(999), "999 B");
        assert_eq!(fmt_bytes(1_500_000), "2 MB");
    }

    #[test]
    fn snooze_hour_and_tomorrow() {
        let off = 2 * 3600; // UTC+2
        let day = 20_000 * 86_400u64; // a UTC midnight
        assert_eq!(snooze_until(Snooze::Hour, day, off), day + 3600);
        // 15:00 local -> next day 08:00 local
        let now = day + 13 * 3600; // 15:00 local
        let until = snooze_until(Snooze::Tomorrow, now, off);
        assert_eq!(until as i64 + off, (day + 86_400 + 8 * 3600) as i64);
        // 01:00 local -> 08:00 on the next calendar day
        let now = day + 86_400 - 3600; // 01:00 local next day
        let until = snooze_until(Snooze::Tomorrow, now, off);
        assert_eq!(until - now, 31 * 3600);
        assert_eq!((until as i64 + off).rem_euclid(86_400), 8 * 3600);
        assert!(is_snoozed(until, now) && !is_snoozed(until, until));
    }

    #[test]
    fn level_maps_to_shape_badge() {
        assert_eq!(badge_for(Level::Ok), Badge::None);
        assert_eq!(badge_for(Level::Warn), Badge::Ring);
        assert_eq!(badge_for(Level::Critical), Badge::Dot);
    }

    #[test]
    fn badge_drawing_touches_only_the_corner() {
        let (w, h) = (44u32, 44u32);
        let glyph = vec![255u8; (w * h * 4) as usize];
        let mut dot = glyph.clone();
        draw_badge(&mut dot, w, h, Badge::Dot);
        let a = |img: &[u8], x: u32, y: u32| img[((y * w + x) * 4 + 3) as usize];
        assert_eq!(a(&dot, 2, 2), 255, "top-left glyph untouched");
        assert_eq!(a(&dot, w - 9, h - 9), 255, "badge centre filled");
        let mut ring = glyph.clone();
        draw_badge(&mut ring, w, h, Badge::Ring);
        assert_eq!(a(&ring, w - 9, h - 9), 0, "ring centre hollow");
        let mut none = glyph.clone();
        draw_badge(&mut none, w, h, Badge::None);
        assert_eq!(none, glyph);
    }
}
