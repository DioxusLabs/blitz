//! The time domain shared by frames and input events.
//!
//! The document never reads a clock itself. The embedder samples its clock at the
//! boundary and passes the result in: as the frame time given to `resolve`, and as the
//! [`timestamp`](crate::events::BlitzPointerEvent::timestamp) of each input event.
//! Both come from the same clock, so an event time and a frame time are directly
//! comparable, and a test can supply whatever times it likes.

use core::ops::{Add, AddAssign, Sub, SubAssign};
use core::time::Duration;
use std::sync::OnceLock;

use web_time::Instant;

/// A moment on the embedder's monotonic clock, in seconds since an arbitrary origin.
///
/// Analogous to a `DOMHighResTimeStamp` (`performance.now()`): only differences between
/// two timestamps carry meaning.
///
/// Timestamps are totally ordered (via [`f64::total_cmp`]) so they can be used as
/// sort keys and in `min`/`max`; they are expected never to be NaN.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timestamp(f64);

impl PartialEq for Timestamp {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Timestamp {}

impl PartialOrd for Timestamp {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Timestamp {
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl Timestamp {
    /// The origin of the time domain.
    pub const ZERO: Timestamp = Timestamp(0.0);

    /// A timestamp `secs` seconds after the origin.
    #[inline]
    pub const fn from_secs_f64(secs: f64) -> Self {
        Self(secs)
    }

    /// Seconds since the origin.
    #[inline]
    pub const fn as_secs_f64(self) -> f64 {
        self.0
    }

    /// Time elapsed from `earlier` to `self`, saturating at zero if `earlier` is later.
    #[inline]
    pub fn duration_since(self, earlier: Timestamp) -> Duration {
        Duration::try_from_secs_f64(self.0 - earlier.0).unwrap_or(Duration::ZERO)
    }
}

impl Add<Duration> for Timestamp {
    type Output = Timestamp;
    #[inline]
    fn add(self, rhs: Duration) -> Timestamp {
        Timestamp(self.0 + rhs.as_secs_f64())
    }
}

impl AddAssign<Duration> for Timestamp {
    #[inline]
    fn add_assign(&mut self, rhs: Duration) {
        self.0 += rhs.as_secs_f64();
    }
}

impl Sub<Duration> for Timestamp {
    type Output = Timestamp;
    #[inline]
    fn sub(self, rhs: Duration) -> Timestamp {
        Timestamp(self.0 - rhs.as_secs_f64())
    }
}

impl SubAssign<Duration> for Timestamp {
    #[inline]
    fn sub_assign(&mut self, rhs: Duration) {
        self.0 -= rhs.as_secs_f64();
    }
}

/// A source of [`Timestamp`]s.
///
/// Implemented by [`SystemClock`] for real time, and by embedders which drive time
/// themselves (test runners, virtual time) with whatever they like.
pub trait Clock {
    /// The current time.
    fn now(&self) -> Timestamp;
}

/// A stopped clock: always reads this timestamp. Useful as the starting point for a
/// virtual clock which is advanced manually.
impl Clock for Timestamp {
    fn now(&self) -> Timestamp {
        *self
    }
}

/// The system's monotonic clock, as a [`Clock`].
///
/// Its origin is fixed on first use and shared by the whole process, so timestamps
/// taken by any `SystemClock` (the shell's frame and event times, script's `Date` and
/// timers) are directly comparable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemClock;

static ORIGIN: OnceLock<Instant> = OnceLock::new();

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let origin = *ORIGIN.get_or_init(Instant::now);
        Timestamp::from_secs_f64(Instant::now().duration_since(origin).as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_since_saturates() {
        let a = Timestamp::from_secs_f64(1.0);
        let b = a + Duration::from_millis(250);
        assert_eq!(b.duration_since(a), Duration::from_millis(250));
        assert_eq!(a.duration_since(b), Duration::ZERO);
    }
}
