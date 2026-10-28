//! The system clock: read here, in the daemon, and nowhere below it.

use cua_desktop::Stamp;
use cua_run::Clock;
use porter_ids::UnixSeconds;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The system's clock, with its origin at construction. Copies share the origin: capture
/// stamps its frames on the same clock the run machine reads.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// A clock whose `Stamp`s count from now.
    pub fn start() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Stamp {
        Stamp(u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX))
    }

    fn unix(&self) -> UnixSeconds {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        UnixSeconds(seconds)
    }

    fn after(&self, limit: std::time::Duration) -> impl std::future::Future<Output = ()> + Send {
        tokio::time::sleep(limit)
    }
}
