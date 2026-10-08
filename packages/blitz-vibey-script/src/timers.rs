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
    /// Whether this is a `requestAnimationFrame` callback.
    pub animation_frame: bool,
}

#[derive(Default)]
pub(crate) struct TimerQueue {
    next_id: u64,
    timers: Vec<Timer>,
    /// When the last frame ran. Only tracked with a virtual clock.
    last_frame: Option<Instant>,
}

/// The time between frames.
const FRAME: Duration = Duration::from_millis(16);

impl TimerQueue {
    pub fn add(
        &mut self,
        now: Instant,
        delay: Duration,
        interval: Option<Duration>,
        callback: JsObject,
        args: Vec<JsValue>,
    ) -> u64 {
        self.push(now + delay, interval, callback, args, false)
    }

    pub fn add_animation_frame(&mut self, now: Instant, callback: JsObject) -> u64 {
        self.push(self.next_frame(now), None, callback, Vec::new(), true)
    }

    /// The time of the next frame: one frame after the last one, or after `now` if no
    /// frame ran recently.
    pub fn next_frame(&self, now: Instant) -> Instant {
        match self.last_frame {
            Some(last_frame) if last_frame + FRAME > now => last_frame + FRAME,
            _ => now + FRAME,
        }
    }

    pub fn frame_started(&mut self, now: Instant) {
        self.last_frame = Some(now);
    }

    fn push(
        &mut self,
        deadline: Instant,
        interval: Option<Duration>,
        callback: JsObject,
        args: Vec<JsValue>,
        animation_frame: bool,
    ) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.timers.push(Timer {
            id,
            deadline,
            interval,
            callback,
            args,
            animation_frame,
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
                    animation_frame: false,
                });
            }
        }

        due.sort_by_key(|timer| timer.deadline);
        due
    }
}
