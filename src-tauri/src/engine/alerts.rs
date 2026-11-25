//! Pure alert state machine: no clock, no I/O. Feed it (usage, now) on every poll.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Level {
    #[default]
    Ok,
    Warn,
    Critical,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
pub struct AlertState {
    pub level: Level,
    pub last_notified: Option<u64>, // unix secs
}

const HYSTERESIS: f32 = 2.0;
const COOLDOWN_SECS: u64 = 24 * 3600;

fn raw_level(pct: f32, warn: u8, crit: u8) -> Level {
    if pct >= crit as f32 { Level::Critical } else if pct >= warn as f32 { Level::Warn } else { Level::Ok }
}

/// Returns (new state, level to notify about if any).
/// - Escalation (ok→warn, warn→critical) notifies immediately.
/// - De-escalation only happens once usage drops HYSTERESIS below the threshold (no flapping).
/// - Staying over a level re-notifies at most once per COOLDOWN.
pub fn step(state: AlertState, used_pct: f32, warn: u8, crit: u8, now: u64) -> (AlertState, Option<Level>) {
    let raw = raw_level(used_pct, warn, crit);
    let held = raw_level(used_pct + HYSTERESIS, warn, crit); // level we'd have with hysteresis applied
    let level = if raw >= state.level { raw } else { held.max(raw).min(state.level) };
    let escalated = level > state.level;
    let due = state.last_notified.is_none_or(|t| now.saturating_sub(t) >= COOLDOWN_SECS);
    let notify = level != Level::Ok && (escalated || due);
    let next = AlertState { level, last_notified: if notify { Some(now) } else { state.last_notified } };
    (next, notify.then_some(level))
}

#[cfg(test)]
mod tests {
    use super::*;
    const D: u64 = 24 * 3600;

    #[test]
    fn escalates_once_then_quiet() {
        let s = AlertState::default();
        let (s, n) = step(s, 81.0, 80, 90, 0);
        assert_eq!(n, Some(Level::Warn));
        let (s, n) = step(s, 82.0, 80, 90, 60);
        assert_eq!(n, None);
        let (_, n) = step(s, 91.0, 80, 90, 120);
        assert_eq!(n, Some(Level::Critical));
    }
    #[test]
    fn hysteresis_prevents_flapping() {
        let (s, _) = step(AlertState::default(), 81.0, 80, 90, 0);
        let (s, n) = step(s, 79.5, 80, 90, 10); // within 2% band: still warn, no notify
        assert_eq!((s.level, n), (Level::Warn, None));
        let (s, _) = step(s, 70.0, 80, 90, 20); // well below: recovers
        assert_eq!(s.level, Level::Ok);
        let (_, n) = step(s, 81.0, 80, 90, 30); // crossing again notifies
        assert_eq!(n, Some(Level::Warn));
    }
    #[test]
    fn cooldown_renotifies_after_a_day() {
        let (s, _) = step(AlertState::default(), 85.0, 80, 90, 0);
        assert_eq!(step(s, 85.0, 80, 90, D - 1).1, None);
        assert_eq!(step(s, 85.0, 80, 90, D).1, Some(Level::Warn));
    }
}
