//! The clock which drives JS-observable time (timers and `Date`).

use std::cell::RefCell;
use std::rc::Rc;

use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The clock used for JS timer deadlines and `Date`.
///
/// In the default real mode it tracks the system's monotonic clock. In
/// virtual mode, time only advances when [`advance_to`](ScriptClock::advance_to)
/// is called: embedders which drive timers manually (e.g. test runners) can
/// jump straight to the next timer deadline instead of sleeping until it,
/// while preserving timer ordering.
#[derive(Clone)]
pub(crate) struct ScriptClock {
    inner: Rc<RefCell<ClockMode>>,
}

enum ClockMode {
    Real,
    Virtual { now: Instant },
}

impl Default for ScriptClock {
    fn default() -> Self {
        Self {
            inner: Rc::new(RefCell::new(ClockMode::Real)),
        }
    }
}

impl ScriptClock {
    /// The current time according to this clock
    pub fn now(&self) -> Instant {
        match *self.inner.borrow() {
            ClockMode::Real => Instant::now(),
            ClockMode::Virtual { now } => now,
        }
    }

    /// Switch to virtual mode. Time stops at the current instant and only
    /// advances via [`advance_to`](Self::advance_to).
    pub fn make_virtual(&self) {
        let mut mode = self.inner.borrow_mut();
        if matches!(*mode, ClockMode::Real) {
            *mode = ClockMode::Virtual {
                now: Instant::now(),
            };
        }
    }

    /// Advance a virtual clock to `deadline` (never backwards).
    /// Does nothing in real mode.
    pub fn advance_to(&self, deadline: Instant) {
        if let ClockMode::Virtual { now } = &mut *self.inner.borrow_mut() {
            *now = (*now).max(deadline);
        }
    }
}

/// Adapter exposing a [`ScriptClock`] as a Boa [`Clock`](boa_engine::context::Clock),
/// so that `Date` observes virtual time consistently with timers.
pub(crate) struct BoaClockAdapter {
    pub clock: ScriptClock,
    /// Anchor for converting `Instant`s to durations-since-an-epoch
    pub base: Instant,
    /// Wall-clock time (ms since the Unix epoch) at `base`
    pub base_system_millis: i64,
}

impl BoaClockAdapter {
    pub fn new(clock: ScriptClock) -> Self {
        let base = clock.now();
        let base_system_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as i64;
        Self {
            clock,
            base,
            base_system_millis,
        }
    }

    fn elapsed(&self) -> Duration {
        self.clock.now().saturating_duration_since(self.base)
    }
}

impl boa_engine::context::Clock for BoaClockAdapter {
    fn now(&self) -> boa_engine::context::time::JsInstant {
        let elapsed = self.elapsed();
        boa_engine::context::time::JsInstant::new(elapsed.as_secs(), elapsed.subsec_nanos())
    }

    fn system_time_millis(&self) -> i64 {
        self.base_system_millis + self.elapsed().as_millis() as i64
    }
}
