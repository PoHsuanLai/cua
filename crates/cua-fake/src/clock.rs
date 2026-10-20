//! A clock that says what it was told.

use cua_desktop::Stamp;
use cua_run::Clock;
use porter_core::UnixSeconds;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

/// A clock set by hand; it never moves on its own.
#[derive(Debug, Default)]
pub struct FixedClock {
    stamp: AtomicU64,
    unix: AtomicI64,
    impatient: AtomicBool,
}

impl FixedClock {
    /// A clock at `stamp` and `unix`.
    pub fn at(stamp: Stamp, unix: UnixSeconds) -> Self {
        Self {
            stamp: AtomicU64::new(stamp.0),
            unix: AtomicI64::new(unix.0),
            impatient: AtomicBool::new(false),
        }
    }

    /// The same clock, whose timers fire at once: whatever waits on one gives up.
    #[must_use]
    pub fn impatient(self) -> Self {
        self.impatient.store(true, Ordering::SeqCst);
        self
    }

    /// Moves both readings forward by `ms` milliseconds.
    pub fn advance(&self, ms: u64) {
        self.stamp.fetch_add(ms, Ordering::SeqCst);
        let seconds = i64::try_from(ms / 1000).unwrap_or(i64::MAX);
        self.unix.fetch_add(seconds, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Stamp {
        Stamp(self.stamp.load(Ordering::SeqCst))
    }

    fn unix(&self) -> UnixSeconds {
        UnixSeconds(self.unix.load(Ordering::SeqCst))
    }

    async fn after(&self, limit: std::time::Duration) {
        let _ = limit;
        if self.impatient.load(Ordering::SeqCst) {
            return;
        }
        std::future::pending::<()>().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_clock_moves_only_when_told() {
        let clock = FixedClock::at(Stamp(1000), UnixSeconds(1_700_000_000));
        assert_eq!(clock.now(), Stamp(1000));
        assert_eq!(clock.now(), Stamp(1000));
        clock.advance(2500);
        assert_eq!(clock.now(), Stamp(3500));
        assert_eq!(clock.unix(), UnixSeconds(1_700_000_002));
    }
}
