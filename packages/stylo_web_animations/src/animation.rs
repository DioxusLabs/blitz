//! The [`Animation`] state machine.
//!
//! <https://drafts.csswg.org/web-animations-1/#animations>

/// Something the host must do in response to a call on an [`Animation`].
///
/// Promises and events are owned by the host. A new animation starts with a resolved ready
/// promise and a pending finished promise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Replace the current ready promise with a new pending promise.
    ReplaceReadyPromise,
    /// Resolve the current ready promise.
    ResolveReadyPromise,
    /// Reject the current ready promise with an `AbortError`, then replace it with a resolved
    /// promise.
    AbortReadyPromise,
    /// Replace the current finished promise with a new pending promise.
    ReplaceFinishedPromise,
    /// Resolve the current finished promise.
    ResolveFinishedPromise,
    /// Reject the current finished promise with an `AbortError`, then replace it with a new
    /// pending promise.
    AbortFinishedPromise,
    /// Queue a microtask that calls [`Animation::run_finish_notification`].
    QueueFinishNotification,
    /// Queue an animation playback event.
    QueueEvent {
        kind: EventKind,
        current_time: Option<f64>,
        timeline_time: Option<f64>,
    },
}

/// The type of an animation playback event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKind {
    Finish,
    Cancel,
}

/// The exception a failed call should throw.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidState,
    Type,
}

/// <https://drafts.csswg.org/web-animations-1/#play-states>
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayState {
    Idle,
    Running,
    Paused,
    Finished,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingTask {
    Play,
    Pause,
}

/// The playback state of an animation.
///
/// Times are in milliseconds and an unresolved time is `None`. Every call that depends on the
/// timeline takes its current time, which is `None` if the animation has no timeline or the
/// timeline is inactive. Calls that may affect promises or events push to `actions`.
#[derive(Clone, Debug)]
pub struct Animation {
    start_time: Option<f64>,
    hold_time: Option<f64>,
    previous_current_time: Option<f64>,
    playback_rate: f64,
    pending_playback_rate: Option<f64>,
    pending_task: Option<PendingTask>,
    effect_end: f64,
    finished_promise_resolved: bool,
    finish_notification_queued: bool,
}

impl Animation {
    /// Creates an idle animation whose associated effect has the given end time. An animation
    /// with no effect has an end time of zero.
    pub fn new(effect_end: f64) -> Self {
        Self {
            start_time: None,
            hold_time: None,
            previous_current_time: None,
            playback_rate: 1.,
            pending_playback_rate: None,
            pending_task: None,
            effect_end,
            finished_promise_resolved: false,
            finish_notification_queued: false,
        }
    }

    pub fn start_time(&self) -> Option<f64> {
        self.start_time
    }

    pub fn playback_rate(&self) -> f64 {
        self.playback_rate
    }

    /// Whether the animation has a pending play task or a pending pause task.
    pub fn pending(&self) -> bool {
        self.pending_task.is_some()
    }

    pub fn effect_end(&self) -> f64 {
        self.effect_end
    }

    /// <https://drafts.csswg.org/web-animations-1/#the-current-time-of-an-animation>
    pub fn current_time(&self, timeline_time: Option<f64>) -> Option<f64> {
        if self.hold_time.is_some() {
            return self.hold_time;
        }
        self.unconstrained_current_time(timeline_time)
    }

    /// The current time calculated as if the hold time were unresolved.
    fn unconstrained_current_time(&self, timeline_time: Option<f64>) -> Option<f64> {
        Some((timeline_time? - self.start_time?) * self.playback_rate)
    }

    /// <https://drafts.csswg.org/web-animations-1/#effective-playback-rate>
    fn effective_playback_rate(&self) -> f64 {
        self.pending_playback_rate.unwrap_or(self.playback_rate)
    }

    /// <https://drafts.csswg.org/web-animations-1/#apply-any-pending-playback-rate>
    fn apply_pending_playback_rate(&mut self) {
        if let Some(rate) = self.pending_playback_rate.take() {
            self.playback_rate = rate;
        }
    }

    /// <https://drafts.csswg.org/web-animations-1/#play-states>
    pub fn play_state(&self, timeline_time: Option<f64>) -> PlayState {
        let current_time = self.current_time(timeline_time);
        if current_time.is_none() && self.start_time.is_none() && self.pending_task.is_none() {
            return PlayState::Idle;
        }
        if self.pending_task == Some(PendingTask::Pause)
            || (self.start_time.is_none() && self.pending_task != Some(PendingTask::Play))
        {
            return PlayState::Paused;
        }
        if let Some(current_time) = current_time {
            let rate = self.effective_playback_rate();
            if (rate > 0. && current_time >= self.effect_end) || (rate < 0. && current_time <= 0.) {
                return PlayState::Finished;
            }
        }
        PlayState::Running
    }

    /// <https://drafts.csswg.org/web-animations-1/#silently-set-the-current-time>
    fn silently_set_current_time(
        &mut self,
        seek_time: Option<f64>,
        timeline_time: Option<f64>,
    ) -> Result<(), Error> {
        let Some(seek_time) = seek_time else {
            if self.current_time(timeline_time).is_some() {
                return Err(Error::Type);
            }
            return Ok(());
        };
        match timeline_time {
            Some(timeline_time)
                if self.hold_time.is_none()
                    && self.start_time.is_some()
                    && self.playback_rate != 0. =>
            {
                self.start_time = Some(timeline_time - seek_time / self.playback_rate);
            }
            _ => self.hold_time = Some(seek_time),
        }
        if timeline_time.is_none() {
            self.start_time = None;
        }
        self.previous_current_time = None;
        Ok(())
    }

    /// <https://drafts.csswg.org/web-animations-1/#setting-the-current-time-of-an-animation>
    pub fn set_current_time(
        &mut self,
        seek_time: Option<f64>,
        timeline_time: Option<f64>,
        actions: &mut Vec<Action>,
    ) -> Result<(), Error> {
        self.silently_set_current_time(seek_time, timeline_time)?;
        if self.pending_task == Some(PendingTask::Pause) {
            self.hold_time = seek_time;
            self.apply_pending_playback_rate();
            self.start_time = None;
            self.pending_task = None;
            actions.push(Action::ResolveReadyPromise);
        }
        self.update_finished_state(true, false, timeline_time, actions);
        Ok(())
    }

    /// <https://drafts.csswg.org/web-animations-1/#setting-the-start-time-of-an-animation>
    pub fn set_start_time(
        &mut self,
        new_start_time: Option<f64>,
        timeline_time: Option<f64>,
        actions: &mut Vec<Action>,
    ) {
        if timeline_time.is_none() && new_start_time.is_some() {
            self.hold_time = None;
        }
        let previous_current_time = self.current_time(timeline_time);
        self.apply_pending_playback_rate();
        self.start_time = new_start_time;
        if new_start_time.is_some() {
            if self.playback_rate != 0. {
                self.hold_time = None;
            }
        } else {
            self.hold_time = previous_current_time;
        }
        if self.pending_task.take().is_some() {
            actions.push(Action::ResolveReadyPromise);
        }
        self.update_finished_state(true, false, timeline_time, actions);
    }

    /// <https://drafts.csswg.org/web-animations-1/#playing-an-animation-section>
    pub fn play(
        &mut self,
        auto_rewind: bool,
        timeline_time: Option<f64>,
        actions: &mut Vec<Action>,
    ) -> Result<(), Error> {
        let aborted_pause = self.pending_task == Some(PendingTask::Pause);
        let rate = self.effective_playback_rate();
        let current_time = self.current_time(timeline_time);
        let end = self.effect_end;
        if rate > 0. && auto_rewind && current_time.is_none_or(|time| time < 0. || time >= end) {
            self.hold_time = Some(0.);
        } else if rate < 0.
            && auto_rewind
            && current_time.is_none_or(|time| time <= 0. || time > end)
        {
            if end.is_infinite() {
                return Err(Error::InvalidState);
            }
            self.hold_time = Some(end);
        } else if current_time.is_none() {
            // The specification only does this for a zero playback rate. Browsers do it
            // whenever the current time is unresolved:
            // https://github.com/w3c/csswg-drafts/issues/7145
            self.hold_time = Some(0.);
        }

        let has_pending_ready_promise = self.pending_task.take().is_some();
        if self.hold_time.is_none() && !aborted_pause && self.pending_playback_rate.is_none() {
            // The animation is already playing, or about to: keep any pending play task so
            // that the ready promise still gets resolved.
            if has_pending_ready_promise {
                self.pending_task = Some(PendingTask::Play);
            }
            return Ok(());
        }
        if self.hold_time.is_some() {
            self.start_time = None;
        }
        if !has_pending_ready_promise {
            actions.push(Action::ReplaceReadyPromise);
        }
        self.pending_task = Some(PendingTask::Play);
        self.update_finished_state(false, false, timeline_time, actions);
        Ok(())
    }

    /// <https://drafts.csswg.org/web-animations-1/#pausing-an-animation-section>
    pub fn pause(
        &mut self,
        timeline_time: Option<f64>,
        actions: &mut Vec<Action>,
    ) -> Result<(), Error> {
        if self.pending_task == Some(PendingTask::Pause)
            || self.play_state(timeline_time) == PlayState::Paused
        {
            return Ok(());
        }
        if self.current_time(timeline_time).is_none() {
            if self.playback_rate >= 0. {
                self.hold_time = Some(0.);
            } else if self.effect_end.is_infinite() {
                return Err(Error::InvalidState);
            } else {
                self.hold_time = Some(self.effect_end);
            }
        }
        if self.pending_task.take().is_none() {
            actions.push(Action::ReplaceReadyPromise);
        }
        self.pending_task = Some(PendingTask::Pause);
        self.update_finished_state(false, false, timeline_time, actions);
        Ok(())
    }

    /// Runs the pending play or pause task, if any. The host calls this once the animation is
    /// [ready](https://drafts.csswg.org/web-animations-1/#ready), with the timeline time at
    /// that moment.
    pub fn run_pending_task(&mut self, ready_time: f64, actions: &mut Vec<Action>) {
        let Some(task) = self.pending_task.take() else {
            return;
        };
        match task {
            PendingTask::Play => {
                debug_assert!(self.start_time.is_some() || self.hold_time.is_some());
                if let Some(hold_time) = self.hold_time {
                    self.apply_pending_playback_rate();
                    if self.playback_rate == 0. {
                        self.start_time = Some(ready_time);
                    } else {
                        self.start_time = Some(ready_time - hold_time / self.playback_rate);
                        self.hold_time = None;
                    }
                } else if let Some(start_time) = self.start_time
                    && self.pending_playback_rate.is_some()
                {
                    let current_time_to_match = (ready_time - start_time) * self.playback_rate;
                    self.apply_pending_playback_rate();
                    if self.playback_rate == 0. {
                        self.hold_time = Some(current_time_to_match);
                        self.start_time = Some(ready_time);
                    } else {
                        self.start_time =
                            Some(ready_time - current_time_to_match / self.playback_rate);
                    }
                }
            }
            PendingTask::Pause => {
                if let Some(start_time) = self.start_time
                    && self.hold_time.is_none()
                {
                    self.hold_time = Some((ready_time - start_time) * self.playback_rate);
                }
                self.apply_pending_playback_rate();
                self.start_time = None;
            }
        }
        actions.push(Action::ResolveReadyPromise);
        self.update_finished_state(false, false, Some(ready_time), actions);
    }

    /// <https://drafts.csswg.org/web-animations-1/#finishing-an-animation-section>
    pub fn finish(
        &mut self,
        timeline_time: Option<f64>,
        actions: &mut Vec<Action>,
    ) -> Result<(), Error> {
        let rate = self.effective_playback_rate();
        if rate == 0. || (rate > 0. && self.effect_end.is_infinite()) {
            return Err(Error::InvalidState);
        }
        self.apply_pending_playback_rate();
        let limit = if self.playback_rate > 0. {
            self.effect_end
        } else {
            0.
        };
        self.silently_set_current_time(Some(limit), timeline_time)?;
        if self.start_time.is_none()
            && let Some(timeline_time) = timeline_time
        {
            self.start_time = Some(timeline_time - limit / self.playback_rate);
        }
        if self.start_time.is_some()
            && let Some(task) = self.pending_task.take()
        {
            if task == PendingTask::Pause {
                self.hold_time = None;
            }
            actions.push(Action::ResolveReadyPromise);
        }
        self.update_finished_state(true, true, timeline_time, actions);
        Ok(())
    }

    /// <https://drafts.csswg.org/web-animations-1/#canceling-an-animation-section>
    pub fn cancel(&mut self, timeline_time: Option<f64>, actions: &mut Vec<Action>) {
        if self.play_state(timeline_time) != PlayState::Idle {
            if self.pending_task.take().is_some() {
                self.apply_pending_playback_rate();
                actions.push(Action::AbortReadyPromise);
            }
            actions.push(Action::AbortFinishedPromise);
            self.finished_promise_resolved = false;
            self.finish_notification_queued = false;
            actions.push(Action::QueueEvent {
                kind: EventKind::Cancel,
                current_time: None,
                timeline_time,
            });
        }
        self.hold_time = None;
        self.start_time = None;
    }

    /// <https://drafts.csswg.org/web-animations-1/#reversing-an-animation-section>
    pub fn reverse(
        &mut self,
        timeline_time: Option<f64>,
        actions: &mut Vec<Action>,
    ) -> Result<(), Error> {
        if timeline_time.is_none() {
            return Err(Error::InvalidState);
        }
        let original_pending_playback_rate = self.pending_playback_rate;
        let rate = self.effective_playback_rate();
        self.pending_playback_rate = Some(if rate == 0. { 0. } else { -rate });
        let result = self.play(true, timeline_time, actions);
        if result.is_err() {
            self.pending_playback_rate = original_pending_playback_rate;
        }
        result
    }

    /// <https://drafts.csswg.org/web-animations-1/#setting-the-playback-rate-of-an-animation>
    pub fn set_playback_rate(
        &mut self,
        new_playback_rate: f64,
        timeline_time: Option<f64>,
        actions: &mut Vec<Action>,
    ) {
        self.pending_playback_rate = None;
        let previous_time = self.current_time(timeline_time);
        self.playback_rate = new_playback_rate;
        if previous_time.is_some() {
            // Cannot fail: the seek time is resolved.
            let _ = self.set_current_time(previous_time, timeline_time, actions);
        }
    }

    /// <https://drafts.csswg.org/web-animations-1/#seamlessly-updating-the-playback-rate-of-an-animation>
    pub fn update_playback_rate(
        &mut self,
        new_playback_rate: f64,
        timeline_time: Option<f64>,
        actions: &mut Vec<Action>,
    ) {
        let previous_play_state = self.play_state(timeline_time);
        self.pending_playback_rate = Some(new_playback_rate);
        if self.pending_task.is_some() {
            return;
        }
        if self.current_time(timeline_time).is_none() {
            self.apply_pending_playback_rate();
            return;
        }
        match previous_play_state {
            PlayState::Idle | PlayState::Paused => self.apply_pending_playback_rate(),
            PlayState::Finished => {
                let unconstrained_current_time = self.unconstrained_current_time(timeline_time);
                self.start_time = if new_playback_rate == 0. {
                    timeline_time
                } else {
                    unconstrained_current_time
                        .and_then(|time| Some(timeline_time? - time / new_playback_rate))
                };
                self.apply_pending_playback_rate();
                self.update_finished_state(false, false, timeline_time, actions);
            }
            PlayState::Running => {
                // Cannot fail: auto-rewind is off.
                let _ = self.play(false, timeline_time, actions);
            }
        }
    }

    /// Sets the end time of the associated effect, after its timing has changed or the effect
    /// has been replaced.
    pub fn set_effect_end(
        &mut self,
        effect_end: f64,
        timeline_time: Option<f64>,
        actions: &mut Vec<Action>,
    ) {
        self.effect_end = effect_end;
        self.update_finished_state(false, false, timeline_time, actions);
    }

    /// Updates the animation for a new timeline time. The host calls this for every animation
    /// when it [updates animations and sends events].
    ///
    /// [updates animations and sends events]: https://drafts.csswg.org/web-animations-1/#update-animations-and-send-events
    pub fn tick(&mut self, timeline_time: Option<f64>, actions: &mut Vec<Action>) {
        self.update_finished_state(false, false, timeline_time, actions);
    }

    /// <https://drafts.csswg.org/web-animations-1/#update-an-animations-finished-state>
    fn update_finished_state(
        &mut self,
        did_seek: bool,
        synchronously_notify: bool,
        timeline_time: Option<f64>,
        actions: &mut Vec<Action>,
    ) {
        let unconstrained_current_time = if did_seek {
            self.current_time(timeline_time)
        } else {
            self.unconstrained_current_time(timeline_time)
        };
        if let Some(unconstrained_current_time) = unconstrained_current_time
            && self.start_time.is_some()
            && self.pending_task.is_none()
        {
            let rate = self.playback_rate;
            let end = self.effect_end;
            if rate > 0. && unconstrained_current_time >= end {
                self.hold_time = Some(if did_seek {
                    unconstrained_current_time
                } else {
                    self.previous_current_time.map_or(end, |time| time.max(end))
                });
            } else if rate < 0. && unconstrained_current_time <= 0. {
                self.hold_time = Some(if did_seek {
                    unconstrained_current_time
                } else {
                    self.previous_current_time.map_or(0., |time| time.min(0.))
                });
            } else if rate != 0.
                && let Some(timeline_time) = timeline_time
            {
                if did_seek && let Some(hold_time) = self.hold_time {
                    self.start_time = Some(timeline_time - hold_time / rate);
                }
                self.hold_time = None;
            }
        }
        self.previous_current_time = self.current_time(timeline_time);

        let finished = self.play_state(timeline_time) == PlayState::Finished;
        if finished && !self.finished_promise_resolved {
            if synchronously_notify {
                self.finish_notification_queued = true;
                self.run_finish_notification(timeline_time, actions);
            } else if !self.finish_notification_queued {
                self.finish_notification_queued = true;
                actions.push(Action::QueueFinishNotification);
            }
        }
        if !finished && self.finished_promise_resolved {
            actions.push(Action::ReplaceFinishedPromise);
            self.finished_promise_resolved = false;
        }
    }

    /// Runs the finish notification steps. The host calls this from the microtask requested by
    /// [`Action::QueueFinishNotification`]. Does nothing if the notification is no longer due.
    pub fn run_finish_notification(
        &mut self,
        timeline_time: Option<f64>,
        actions: &mut Vec<Action>,
    ) {
        if !std::mem::take(&mut self.finish_notification_queued) {
            return;
        }
        if self.play_state(timeline_time) != PlayState::Finished {
            return;
        }
        actions.push(Action::ResolveFinishedPromise);
        self.finished_promise_resolved = true;
        actions.push(Action::QueueEvent {
            kind: EventKind::Finish,
            current_time: self.current_time(timeline_time),
            timeline_time,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::*;

    /// A running animation with a start time of 0 and an effect end of 100.
    fn running(actions: &mut Vec<Action>) -> Animation {
        let mut animation = Animation::new(100.);
        animation.play(true, Some(0.), actions).unwrap();
        animation.run_pending_task(0., actions);
        actions.clear();
        animation
    }

    fn finish_event(current_time: f64, timeline_time: f64) -> Action {
        QueueEvent {
            kind: EventKind::Finish,
            current_time: Some(current_time),
            timeline_time: Some(timeline_time),
        }
    }

    #[test]
    fn new_animation_is_idle() {
        let animation = Animation::new(100.);
        assert_eq!(animation.play_state(Some(0.)), PlayState::Idle);
        assert_eq!(animation.current_time(Some(0.)), None);
        assert!(!animation.pending());
    }

    #[test]
    fn play_is_pending_until_ready() {
        let mut actions = Vec::new();
        let mut animation = Animation::new(100.);
        animation.play(true, Some(10.), &mut actions).unwrap();
        assert_eq!(actions, [ReplaceReadyPromise]);
        assert!(animation.pending());
        assert_eq!(animation.play_state(Some(10.)), PlayState::Running);
        assert_eq!(animation.start_time(), None);
        assert_eq!(animation.current_time(Some(15.)), Some(0.));

        actions.clear();
        animation.run_pending_task(20., &mut actions);
        assert_eq!(actions, [ResolveReadyPromise]);
        assert!(!animation.pending());
        assert_eq!(animation.start_time(), Some(20.));
        assert_eq!(animation.current_time(Some(50.)), Some(30.));
    }

    #[test]
    fn finishes_when_timeline_passes_the_end() {
        let mut actions = Vec::new();
        let mut animation = running(&mut actions);

        animation.tick(Some(50.), &mut actions);
        assert_eq!(actions, []);
        assert_eq!(animation.play_state(Some(50.)), PlayState::Running);

        animation.tick(Some(120.), &mut actions);
        assert_eq!(actions, [QueueFinishNotification]);
        assert_eq!(animation.play_state(Some(120.)), PlayState::Finished);
        assert_eq!(animation.current_time(Some(120.)), Some(100.));

        // The notification is only queued once.
        actions.clear();
        animation.tick(Some(130.), &mut actions);
        assert_eq!(actions, []);

        animation.run_finish_notification(Some(130.), &mut actions);
        assert_eq!(actions, [ResolveFinishedPromise, finish_event(100., 130.)]);

        actions.clear();
        animation.run_finish_notification(Some(130.), &mut actions);
        animation.tick(Some(140.), &mut actions);
        assert_eq!(actions, []);
    }

    #[test]
    fn finish_notification_is_dropped_if_no_longer_finished() {
        let mut actions = Vec::new();
        let mut animation = running(&mut actions);
        animation.tick(Some(120.), &mut actions);
        animation
            .set_current_time(Some(50.), Some(120.), &mut actions)
            .unwrap();
        actions.clear();
        animation.run_finish_notification(Some(120.), &mut actions);
        assert_eq!(actions, []);
        assert_eq!(animation.play_state(Some(120.)), PlayState::Running);
    }

    #[test]
    fn play_rewinds_a_finished_animation() {
        let mut actions = Vec::new();
        let mut animation = running(&mut actions);
        animation.finish(Some(10.), &mut actions).unwrap();
        assert_eq!(actions, [ResolveFinishedPromise, finish_event(100., 10.)]);
        assert_eq!(animation.start_time(), Some(-90.));

        actions.clear();
        animation.play(true, Some(10.), &mut actions).unwrap();
        assert_eq!(actions, [ReplaceReadyPromise, ReplaceFinishedPromise]);
        assert_eq!(animation.current_time(Some(10.)), Some(0.));

        // Without auto-rewind the animation stays finished.
        let mut animation = running(&mut actions);
        animation.finish(Some(10.), &mut actions).unwrap();
        actions.clear();
        animation.play(false, Some(10.), &mut actions).unwrap();
        assert_eq!(actions, [ReplaceReadyPromise]);
        animation.run_pending_task(20., &mut actions);
        assert_eq!(animation.play_state(Some(20.)), PlayState::Finished);
        assert_eq!(animation.current_time(Some(20.)), Some(100.));
    }

    #[test]
    fn pause_holds_the_current_time() {
        let mut actions = Vec::new();
        let mut animation = running(&mut actions);
        animation.pause(Some(30.), &mut actions).unwrap();
        assert_eq!(actions, [ReplaceReadyPromise]);
        assert!(animation.pending());
        assert_eq!(animation.play_state(Some(30.)), PlayState::Paused);
        // The animation keeps advancing until the pause takes effect.
        assert_eq!(animation.current_time(Some(35.)), Some(35.));

        actions.clear();
        animation.run_pending_task(40., &mut actions);
        assert_eq!(actions, [ResolveReadyPromise]);
        assert_eq!(animation.start_time(), None);
        assert_eq!(animation.current_time(Some(90.)), Some(40.));

        // Pausing again does nothing.
        actions.clear();
        animation.pause(Some(90.), &mut actions).unwrap();
        assert_eq!(actions, []);

        animation.play(true, Some(90.), &mut actions).unwrap();
        animation.run_pending_task(100., &mut actions);
        assert_eq!(animation.start_time(), Some(60.));
        assert_eq!(animation.current_time(Some(110.)), Some(50.));
    }

    #[test]
    fn pause_of_an_idle_animation_holds_at_the_start() {
        let mut actions = Vec::new();
        let mut animation = Animation::new(100.);
        animation.pause(Some(0.), &mut actions).unwrap();
        assert_eq!(animation.current_time(Some(0.)), Some(0.));

        let mut animation = Animation::new(100.);
        animation.set_playback_rate(-1., Some(0.), &mut actions);
        animation.pause(Some(0.), &mut actions).unwrap();
        assert_eq!(animation.current_time(Some(0.)), Some(100.));

        let mut animation = Animation::new(f64::INFINITY);
        animation.set_playback_rate(-1., Some(0.), &mut actions);
        assert_eq!(
            animation.pause(Some(0.), &mut actions),
            Err(Error::InvalidState)
        );
    }

    #[test]
    fn play_aborts_a_pending_pause() {
        let mut actions = Vec::new();
        let mut animation = running(&mut actions);
        animation.pause(Some(30.), &mut actions).unwrap();
        actions.clear();
        animation.play(true, Some(30.), &mut actions).unwrap();
        // The ready promise of the pause is reused.
        assert_eq!(actions, []);
        assert_eq!(animation.play_state(Some(30.)), PlayState::Running);

        animation.run_pending_task(40., &mut actions);
        assert_eq!(actions, [ResolveReadyPromise]);
        assert_eq!(animation.start_time(), Some(0.));
    }

    #[test]
    fn seeking_while_pause_pending_completes_the_pause() {
        let mut actions = Vec::new();
        let mut animation = running(&mut actions);
        animation.pause(Some(30.), &mut actions).unwrap();
        actions.clear();
        animation
            .set_current_time(Some(70.), Some(30.), &mut actions)
            .unwrap();
        assert_eq!(actions, [ResolveReadyPromise]);
        assert!(!animation.pending());
        assert_eq!(animation.play_state(Some(30.)), PlayState::Paused);
        assert_eq!(animation.current_time(Some(50.)), Some(70.));
    }

    #[test]
    fn set_current_time() {
        let mut actions = Vec::new();
        let mut animation = running(&mut actions);
        animation
            .set_current_time(Some(60.), Some(10.), &mut actions)
            .unwrap();
        assert_eq!(animation.start_time(), Some(-50.));
        assert_eq!(animation.current_time(Some(20.)), Some(70.));
        assert_eq!(
            animation.set_current_time(None, Some(20.), &mut actions),
            Err(Error::Type)
        );
        assert_eq!(actions, []);

        // Seeking past the end finishes the animation without clamping the current time.
        animation
            .set_current_time(Some(150.), Some(20.), &mut actions)
            .unwrap();
        assert_eq!(actions, [QueueFinishNotification]);
        assert_eq!(animation.current_time(Some(30.)), Some(150.));

        let mut animation = Animation::new(100.);
        assert_eq!(
            animation.set_current_time(None, Some(0.), &mut actions),
            Ok(())
        );
        animation
            .set_current_time(Some(40.), Some(0.), &mut actions)
            .unwrap();
        assert_eq!(animation.play_state(Some(0.)), PlayState::Paused);
        assert_eq!(animation.current_time(Some(10.)), Some(40.));
    }

    #[test]
    fn set_start_time() {
        let mut actions = Vec::new();
        let mut animation = Animation::new(100.);
        animation.play(true, Some(0.), &mut actions).unwrap();
        actions.clear();
        animation.set_start_time(Some(-20.), Some(0.), &mut actions);
        assert_eq!(actions, [ResolveReadyPromise]);
        assert!(!animation.pending());
        assert_eq!(animation.current_time(Some(10.)), Some(30.));

        // An unresolved start time holds the current time.
        animation.set_start_time(None, Some(10.), &mut actions);
        assert_eq!(animation.play_state(Some(10.)), PlayState::Paused);
        assert_eq!(animation.current_time(Some(50.)), Some(30.));
    }

    #[test]
    fn finish() {
        let mut actions = Vec::new();
        let mut animation = Animation::new(100.);
        animation.play(true, Some(0.), &mut actions).unwrap();
        actions.clear();
        animation.finish(Some(5.), &mut actions).unwrap();
        assert_eq!(
            actions,
            [
                ResolveReadyPromise,
                ResolveFinishedPromise,
                finish_event(100., 5.)
            ]
        );
        assert_eq!(animation.play_state(Some(5.)), PlayState::Finished);

        let mut animation = running(&mut actions);
        animation.set_playback_rate(-1., Some(50.), &mut actions);
        actions.clear();
        animation.finish(Some(60.), &mut actions).unwrap();
        assert_eq!(actions, [ResolveFinishedPromise, finish_event(0., 60.)]);

        let mut animation = Animation::new(f64::INFINITY);
        assert_eq!(
            animation.finish(Some(0.), &mut actions),
            Err(Error::InvalidState)
        );
        let mut animation = Animation::new(100.);
        animation.set_playback_rate(0., Some(0.), &mut actions);
        assert_eq!(
            animation.finish(Some(0.), &mut actions),
            Err(Error::InvalidState)
        );
    }

    #[test]
    fn finish_without_an_active_timeline_does_not_notify() {
        let mut actions = Vec::new();
        let mut animation = Animation::new(100.);
        animation.finish(None, &mut actions).unwrap();
        assert_eq!(actions, []);
        assert_eq!(animation.play_state(None), PlayState::Paused);
        assert_eq!(animation.current_time(None), Some(100.));
    }

    #[test]
    fn cancel() {
        let mut actions = Vec::new();
        let cancel_event = QueueEvent {
            kind: EventKind::Cancel,
            current_time: None,
            timeline_time: Some(30.),
        };

        let mut animation = running(&mut actions);
        animation.cancel(Some(30.), &mut actions);
        assert_eq!(actions, [AbortFinishedPromise, cancel_event]);
        assert_eq!(animation.play_state(Some(30.)), PlayState::Idle);
        assert_eq!(animation.current_time(Some(30.)), None);

        // Cancelling an idle animation does nothing.
        actions.clear();
        animation.cancel(Some(30.), &mut actions);
        assert_eq!(actions, []);

        animation.play(true, Some(30.), &mut actions).unwrap();
        animation.update_playback_rate(2., Some(30.), &mut actions);
        actions.clear();
        animation.cancel(Some(30.), &mut actions);
        assert_eq!(
            actions,
            [AbortReadyPromise, AbortFinishedPromise, cancel_event]
        );
        assert!(!animation.pending());
        assert_eq!(animation.playback_rate(), 2.);
    }

    #[test]
    fn cancel_after_finishing_replaces_the_finished_promise() {
        let mut actions = Vec::new();
        let mut animation = running(&mut actions);
        animation.finish(Some(10.), &mut actions).unwrap();
        animation.cancel(Some(10.), &mut actions);
        actions.clear();

        // The new finished promise is pending, so finishing again resolves it.
        animation.play(true, Some(10.), &mut actions).unwrap();
        animation.finish(Some(10.), &mut actions).unwrap();
        assert!(actions.contains(&ResolveFinishedPromise));
        assert!(!actions.contains(&ReplaceFinishedPromise));
    }

    #[test]
    fn reverse() {
        let mut actions = Vec::new();
        let mut animation = running(&mut actions);
        animation.reverse(Some(40.), &mut actions).unwrap();
        assert_eq!(actions, [ReplaceReadyPromise]);
        // The new rate applies once the animation is ready.
        assert_eq!(animation.playback_rate(), 1.);
        assert_eq!(animation.current_time(Some(45.)), Some(45.));

        animation.run_pending_task(50., &mut actions);
        assert_eq!(animation.playback_rate(), -1.);
        assert_eq!(animation.current_time(Some(50.)), Some(50.));
        assert_eq!(animation.current_time(Some(60.)), Some(40.));

        actions.clear();
        animation.tick(Some(110.), &mut actions);
        assert_eq!(actions, [QueueFinishNotification]);
        assert_eq!(animation.current_time(Some(110.)), Some(0.));
    }

    #[test]
    fn reverse_errors_leave_the_playback_rate_unchanged() {
        let mut actions = Vec::new();
        let mut animation = Animation::new(100.);
        assert_eq!(
            animation.reverse(None, &mut actions),
            Err(Error::InvalidState)
        );

        let mut animation = Animation::new(f64::INFINITY);
        assert_eq!(
            animation.reverse(Some(0.), &mut actions),
            Err(Error::InvalidState)
        );
        animation.play(true, Some(0.), &mut actions).unwrap();
        animation.run_pending_task(0., &mut actions);
        assert_eq!(animation.playback_rate(), 1.);
    }

    #[test]
    fn reverse_from_idle_starts_at_the_end() {
        let mut actions = Vec::new();
        let mut animation = Animation::new(100.);
        animation.reverse(Some(0.), &mut actions).unwrap();
        animation.run_pending_task(0., &mut actions);
        assert_eq!(animation.current_time(Some(10.)), Some(90.));
    }

    #[test]
    fn set_playback_rate_preserves_the_current_time() {
        let mut actions = Vec::new();
        let mut animation = running(&mut actions);
        animation.set_playback_rate(2., Some(30.), &mut actions);
        assert_eq!(actions, []);
        assert_eq!(animation.current_time(Some(30.)), Some(30.));
        assert_eq!(animation.current_time(Some(40.)), Some(50.));

        // A zero rate freezes the animation at its current time.
        animation.set_playback_rate(0., Some(40.), &mut actions);
        assert_eq!(animation.current_time(Some(90.)), Some(50.));
        assert_eq!(animation.play_state(Some(90.)), PlayState::Running);
    }

    #[test]
    fn update_playback_rate() {
        let mut actions = Vec::new();

        // Running: applied when the animation is next ready, preserving the current time.
        let mut animation = running(&mut actions);
        animation.update_playback_rate(2., Some(30.), &mut actions);
        assert_eq!(actions, [ReplaceReadyPromise]);
        assert_eq!(animation.playback_rate(), 1.);
        animation.run_pending_task(40., &mut actions);
        assert_eq!(animation.playback_rate(), 2.);
        assert_eq!(animation.current_time(Some(40.)), Some(40.));
        assert_eq!(animation.current_time(Some(50.)), Some(60.));

        // Idle: applied immediately.
        actions.clear();
        let mut animation = Animation::new(100.);
        animation.update_playback_rate(2., Some(0.), &mut actions);
        assert_eq!(actions, []);
        assert_eq!(animation.playback_rate(), 2.);

        // Finished: applied immediately, and reversing leaves the finished state.
        let mut animation = running(&mut actions);
        animation.finish(Some(10.), &mut actions).unwrap();
        actions.clear();
        animation.update_playback_rate(-1., Some(10.), &mut actions);
        assert_eq!(actions, [ReplaceFinishedPromise]);
        assert_eq!(animation.play_state(Some(10.)), PlayState::Running);
        assert_eq!(animation.current_time(Some(10.)), Some(100.));
        assert_eq!(animation.current_time(Some(30.)), Some(80.));
    }

    #[test]
    fn set_effect_end() {
        let mut actions = Vec::new();
        let mut animation = running(&mut actions);
        animation.set_effect_end(20., Some(50.), &mut actions);
        assert_eq!(actions, [QueueFinishNotification]);
        assert_eq!(animation.play_state(Some(50.)), PlayState::Finished);

        // Extending the effect before the notification runs resumes the animation.
        actions.clear();
        animation.set_effect_end(200., Some(60.), &mut actions);
        animation.run_finish_notification(Some(60.), &mut actions);
        assert_eq!(actions, []);
        assert_eq!(animation.play_state(Some(60.)), PlayState::Running);
        assert_eq!(animation.current_time(Some(60.)), Some(60.));
    }

    #[test]
    fn inactive_timeline() {
        let mut actions = Vec::new();
        let mut animation = Animation::new(100.);
        animation.play(true, None, &mut actions).unwrap();
        assert!(animation.pending());
        assert_eq!(animation.current_time(None), Some(0.));

        let mut animation = running(&mut actions);
        assert_eq!(animation.current_time(None), None);
        assert_eq!(animation.play_state(None), PlayState::Running);
        animation.tick(None, &mut actions);
        assert_eq!(actions, []);
    }
}
