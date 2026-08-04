//! Fixed-tick simulation clock (DESIGN.md §8.3, §12).
//!
//! 1 tick = 1 in-game hour. The clock is a pure counter: it contains no time
//! source, so advancing it is deterministic by construction.

use serde::{Deserialize, Serialize};

/// Simulation time, measured in ticks.
///
/// Invariants:
/// - `1 tick == 1 in-game hour` (DESIGN.md §12 decisions log).
/// - Tick 0 is midnight at the start of in-game day 1 (hour-of-day 0).
/// - `advance*` saturates at `u64::MAX`; the clock never wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Tick(u64);

impl Tick {
    /// Start of the expedition: in-game day 1, hour 0 (midnight).
    pub const START: Tick = Tick(0);

    /// Hours per in-game day.
    pub const HOURS_PER_DAY: u64 = 24;

    /// Build a tick from a raw hour count.
    pub fn from_hours(hours: u64) -> Self {
        Tick(hours)
    }

    /// Advance by one in-game hour. Saturates at `u64::MAX`.
    pub fn advance(&mut self) {
        self.0 = self.0.saturating_add(1);
    }

    /// Advance by `hours` in-game hours. Saturates at `u64::MAX`.
    pub fn advance_by(&mut self, hours: u64) {
        self.0 = self.0.saturating_add(hours);
    }

    /// Total in-game hours elapsed since tick zero.
    pub fn as_hours(self) -> u64 {
        self.0
    }

    /// In-game day, 1-based.
    pub fn day(self) -> u64 {
        self.0 / Self::HOURS_PER_DAY + 1
    }

    /// In-game month, 1–12 (30 days per month, no year rollover for the
    /// expedition's timescale).
    pub fn month(self) -> u32 {
        ((self.day() - 1) / 30 + 1) as u32
    }

    /// Hour of day, 0..24, with 0 = midnight.
    pub fn hour_of_day(self) -> u64 {
        self.0 % Self::HOURS_PER_DAY
    }

    /// True when the hour-of-day is in daylight (06:00 through 17:59).
    pub fn is_daylight(self) -> bool {
        (6..18).contains(&self.hour_of_day())
    }
}

impl Default for Tick {
    fn default() -> Self {
        Tick::START
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_is_day_one_midnight() {
        assert_eq!(Tick::START.as_hours(), 0);
        assert_eq!(Tick::START.day(), 1);
        assert_eq!(Tick::START.hour_of_day(), 0);
    }

    #[test]
    fn advance_increments_by_one_hour() {
        let mut t = Tick::START;
        t.advance();
        assert_eq!(t.as_hours(), 1);
    }

    #[test]
    fn advance_by_hours() {
        let mut t = Tick::START;
        t.advance_by(47);
        assert_eq!(t.as_hours(), 47);
        t.advance();
        assert_eq!(t.as_hours(), 48);
    }

    #[test]
    fn day_rollover_happens_at_hour_24() {
        assert_eq!(Tick::from_hours(23).day(), 1);
        assert_eq!(Tick::from_hours(24).day(), 2);
        assert_eq!(Tick::from_hours(48).day(), 3);
    }

    #[test]
    fn hour_of_day_wraps() {
        assert_eq!(Tick::from_hours(24).hour_of_day(), 0);
        assert_eq!(Tick::from_hours(30).hour_of_day(), 6);
    }

    #[test]
    fn month_derivation() {
        assert_eq!(Tick::START.month(), 1);
        assert_eq!(Tick::from_hours(24 * 29).month(), 1); // day 30
        assert_eq!(Tick::from_hours(24 * 30).month(), 2); // day 31
        assert_eq!(Tick::from_hours(24 * 270).month(), 10);
    }

    #[test]
    fn daylight_derivation() {
        assert!(!Tick::from_hours(0).is_daylight()); // midnight
        assert!(!Tick::from_hours(5).is_daylight()); // 05:00
        assert!(Tick::from_hours(6).is_daylight()); // 06:00
        assert!(Tick::from_hours(17).is_daylight()); // 17:00
        assert!(!Tick::from_hours(18).is_daylight()); // 18:00
    }

    #[test]
    fn advance_saturates_at_u64_max() {
        let mut t = Tick(u64::MAX);
        t.advance();
        assert_eq!(t.as_hours(), u64::MAX);
        t.advance_by(10);
        assert_eq!(t.as_hours(), u64::MAX);
    }
}
