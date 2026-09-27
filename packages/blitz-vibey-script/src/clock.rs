//! The clock which drives JS-observable time (timers, `Date`, `Event.timeStamp`).

use std::cell::RefCell;
use std::rc::Rc;

use blitz_traits::time::{Clock, SystemClock, Timestamp};
use web_time::{Duration, SystemTime, UNIX_EPOCH};

/// The clock used for JS timer deadlines and `Date`.
///
/// It reports [`Timestamp`]s in the same domain as the frame times passed to
/// `resolve` and the timestamps carried by input events, so script-observable
/// time (`performance.now()`, `Event.timeStamp`) agrees with the engine's.
///
/// In the default real mode it tracks the embedder's clock ([`SystemClock`]
/// unless another is supplied). In virtual mode, time only advances when
/// [`advance_to`](ScriptClock::advance_to) is called: embedders which drive
/// timers manually (e.g. test runners) can jump straight to the next timer
/// deadline instead of sleeping until it, while preserving timer ordering.
#[derive(Clone)]
pub(crate) struct ScriptClock {
    inner: Rc<RefCell<ClockMode>>,
    /// The document's time origin (`performance.timeOrigin`): the time the clock
    /// was created. `performance.now()` and `Event.timeStamp` are relative to it.
    origin: Timestamp,
}

enum ClockMode {
    Real(Box<dyn Clock>),
    Virtual { now: Timestamp },
}

impl Default for ScriptClock {
    fn default() -> Self {
        Self::new(SystemClock)
    }
}

impl ScriptClock {
    /// A clock in real mode, reading from `source`.
    pub fn new(source: impl Clock + 'static) -> Self {
        let origin = source.now();
        Self {
            inner: Rc::new(RefCell::new(ClockMode::Real(Box::new(source)))),
            origin,
        }
    }

    /// The current time according to this clock
    pub fn now(&self) -> Timestamp {
        match &*self.inner.borrow() {
            ClockMode::Real(source) => source.now(),
            ClockMode::Virtual { now } => *now,
        }
    }

    /// Time elapsed since the clock's origin
    pub fn elapsed(&self) -> Duration {
        self.now().duration_since(self.origin)
    }

    /// `timestamp` as a `DOMHighResTimeStamp`: milliseconds since the document's
    /// time origin.
    pub fn high_res_millis(&self, timestamp: Timestamp) -> f64 {
        timestamp.duration_since(self.origin).as_secs_f64() * 1000.0
    }

    /// Switch to virtual mode. Time stops at the current instant and only
    /// advances via [`advance_to`](Self::advance_to).
    pub fn make_virtual(&self) {
        let mut mode = self.inner.borrow_mut();
        if let ClockMode::Real(source) = &*mode {
            let now = source.now();
            *mode = ClockMode::Virtual { now };
        }
    }

    /// Advance a virtual clock to `deadline` (never backwards).
    /// Does nothing in real mode.
    pub fn advance_to(&self, deadline: Timestamp) {
        if let ClockMode::Virtual { now } = &mut *self.inner.borrow_mut()
            && deadline > *now
        {
            *now = deadline;
        }
    }
}

/// Adapter exposing a [`ScriptClock`] as a Boa [`Clock`](boa_engine::context::Clock),
/// so that `Date` observes virtual time consistently with timers.
pub(crate) struct BoaClockAdapter {
    pub clock: ScriptClock,
    /// Wall-clock time (ms since the Unix epoch) at the clock's origin
    pub origin_system_millis: i64,
}

impl BoaClockAdapter {
    pub fn new(clock: ScriptClock) -> Self {
        let now_system_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as i64;
        let origin_system_millis = now_system_millis - clock.elapsed().as_millis() as i64;
        Self {
            clock,
            origin_system_millis,
        }
    }

    /// Time elapsed since the clock's origin
    fn elapsed(&self) -> Duration {
        self.clock.elapsed()
    }
}

impl boa_engine::context::Clock for BoaClockAdapter {
    fn now(&self) -> boa_engine::context::time::JsInstant {
        let elapsed = self.elapsed();
        boa_engine::context::time::JsInstant::new(elapsed.as_secs(), elapsed.subsec_nanos())
    }

    fn system_time_millis(&self) -> i64 {
        self.origin_system_millis + self.elapsed().as_millis() as i64
    }
}
