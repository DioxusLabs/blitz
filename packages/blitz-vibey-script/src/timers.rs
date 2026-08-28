//! Timer support (`setTimeout` / `setInterval` / `requestAnimationFrame`)

use boa_engine::JsValue;
use boa_engine::object::JsObject;
use web_time::{Duration, Instant};

pub(crate) struct Timer {
    pub id: u64,
    pub deadline: Instant,
    /// `Some` for `setInterval` timers, which reschedule themselves.
    pub interval: Option<Duration>,
    pub callback: JsObject,
    pub args: Vec<JsValue>,
}

#[derive(Default)]
pub(crate) struct TimerQueue {
    next_id: u64,
    timers: Vec<Timer>,
}

impl TimerQueue {
    pub fn add(
        &mut self,
        delay: Duration,
        interval: Option<Duration>,
        callback: JsObject,
        args: Vec<JsValue>,
    ) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.timers.push(Timer {
            id,
            deadline: Instant::now() + delay,
            interval,
            callback,
            args,
        });
        id
    }

    pub fn remove(&mut self, id: u64) {
        self.timers.retain(|timer| timer.id != id);
    }

    /// The deadline of the timer which is due soonest (if any)
    pub fn next_deadline(&self) -> Option<Instant> {
        self.timers.iter().map(|timer| timer.deadline).min()
    }

    /// Remove and return all timers that are due at `now`, soonest first.
    /// Interval timers are rescheduled.
    pub fn take_due(&mut self, now: Instant) -> Vec<Timer> {
        let mut due: Vec<Timer> = Vec::new();
        let mut idx = 0;
        while idx < self.timers.len() {
            if self.timers[idx].deadline <= now {
                due.push(self.timers.swap_remove(idx));
            } else {
                idx += 1;
            }
        }

        // Reschedule interval timers
        for timer in &due {
            if let Some(interval) = timer.interval {
                self.timers.push(Timer {
                    id: timer.id,
                    deadline: now + interval.max(Duration::from_millis(1)),
                    interval: timer.interval,
                    callback: timer.callback.clone(),
                    args: timer.args.clone(),
                });
            }
        }

        due.sort_by_key(|timer| timer.deadline);
        due
    }
}
