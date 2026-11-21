use blitz_traits::{
    events::{BlitzWheelEvent, BlitzWheelPhase, BlitzWheelSource},
    node_id::NodeId,
};
use web_time::{Duration, Instant};

use crate::{
    BaseDocument,
    scrolling::{ScrollOverflow, ScrollTarget},
    util::Point,
};

/// The axes a wheel event's default scroll moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WheelAxes {
    #[default]
    Both,
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default)]
enum WheelGesturePhase {
    #[default]
    Idle,
    Active {
        recent: (f64, f64),
    },
    Lifted {
        recent: (f64, f64),
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LatchMode {
    Ungestured,
    Gesture { ends: bool },
    Wheel,
}

/// Locks a trackpad scroll gesture to its main axis and latched scroll targets, as browsers do.
///
/// Each event is axis-locked by the gesture's recent movement, as in Chromium: to an axis while
/// the movement mostly follows it, and to neither while it is diagonal. Once a gesture (or a
/// series of stationary discrete wheel ticks) latches onto a target scroller, subsequent deltas
/// stay clamped to that target until the gesture ends, times out, or the pointer moves.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WheelGesture {
    phase: WheelGesturePhase,
    latched_x: Option<ScrollTarget>,
    latched_y: Option<ScrollTarget>,
    wheel_target: Option<NodeId>,
    wheel_latch: Option<(Instant, f32, f32)>,
    last_pointer: Option<(f32, f32)>,
}

impl WheelGesture {
    /// An event locks to an axis when the recent movement along that axis is at least this
    /// many times the movement along the other.
    const LOCK_RATIO: f64 = 2.0;
    /// How much of the recent movement is kept each time an event arrives.
    const DECAY: f64 = 0.8;
    const WHEEL_LATCH_TIMEOUT: Duration = Duration::from_millis(500);
    const POINTER_MOVE_THRESHOLD: f32 = 0.5;

    fn clear_scroll_latch(&mut self) {
        self.latched_x = None;
        self.latched_y = None;
        self.wheel_latch = None;
    }

    pub(crate) fn clear_target_latch(&mut self) {
        self.clear_scroll_latch();
        self.wheel_target = None;
    }

    pub(crate) fn clear_latched_node(&mut self, node_id: NodeId) {
        let target = Some(ScrollTarget::Node(node_id));
        if self.latched_x == target {
            self.latched_x = None;
        }
        if self.latched_y == target {
            self.latched_y = None;
        }
        if self.wheel_target == Some(node_id) {
            self.wheel_target = None;
        }
    }

    pub(crate) fn on_pointer_move(&mut self, x: f32, y: f32) {
        match self.last_pointer {
            Some((px, py))
                if (x - px).abs() <= Self::POINTER_MOVE_THRESHOLD
                    && (y - py).abs() <= Self::POINTER_MOVE_THRESHOLD => {}
            Some(_) => {
                self.clear_target_latch();
                self.last_pointer = Some((x, y));
            }
            None => {
                self.last_pointer = Some((x, y));
            }
        }
    }

    fn is_wheel_burst_active(&self, now: Instant, cx: f32, cy: f32) -> bool {
        self.wheel_latch.is_some_and(|(t, lx, ly)| {
            now.saturating_duration_since(t) <= Self::WHEEL_LATCH_TIMEOUT
                && (cx - lx).abs() <= Self::POINTER_MOVE_THRESHOLD
                && (cy - ly).abs() <= Self::POINTER_MOVE_THRESHOLD
        })
    }

    pub(crate) fn has_active_latch_for(&self, doc: &BaseDocument, event: &BlitzWheelEvent) -> bool {
        let has_valid_x = self
            .latched_x
            .is_some_and(|t| doc.is_valid_scroll_target(t));
        let has_valid_y = self
            .latched_y
            .is_some_and(|t| doc.is_valid_scroll_target(t));

        if event.source == BlitzWheelSource::Wheel {
            if !matches!(self.phase, WheelGesturePhase::Idle) {
                return false;
            }
            if !self.is_wheel_burst_active(Instant::now(), event.client_x(), event.client_y()) {
                return false;
            }
            let (raw_x, raw_y) = event.delta.to_pixels(WHEEL_LINE_PX);
            return (raw_x != 0.0 && has_valid_x) || (raw_y != 0.0 && has_valid_y);
        }

        let continuing_gesture = match self.phase {
            WheelGesturePhase::Active { .. } => event.phase != BlitzWheelPhase::Started,
            WheelGesturePhase::Lifted { .. } => {
                event.phase == BlitzWheelPhase::Started
                    && event.source == BlitzWheelSource::Momentum
            }
            WheelGesturePhase::Idle => false,
        };
        continuing_gesture && (has_valid_x || has_valid_y)
    }

    pub(crate) fn begin_wheel_dispatch(
        &mut self,
        doc: &BaseDocument,
        event: &BlitzWheelEvent,
        fallback_target: NodeId,
    ) -> (NodeId, bool) {
        let cancelable = !self.has_active_latch_for(doc, event);
        let now = Instant::now();
        let (cx, cy) = (event.client_x(), event.client_y());

        if event.source == BlitzWheelSource::Wheel {
            if !matches!(self.phase, WheelGesturePhase::Idle) {
                self.phase = WheelGesturePhase::Idle;
                self.clear_target_latch();
            }
            if self.is_wheel_burst_active(now, cx, cy) {
                if let Some((_, lx, ly)) = self.wheel_latch {
                    self.wheel_latch = Some((now, lx, ly));
                }
            } else {
                self.clear_target_latch();
                self.wheel_target = Some(fallback_target);
                self.wheel_latch = Some((now, cx, cy));
                self.last_pointer = Some((cx, cy));
            }
        } else {
            let started = event.phase == BlitzWheelPhase::Started;
            let continuing_momentum = matches!(self.phase, WheelGesturePhase::Lifted { .. })
                && started
                && event.source == BlitzWheelSource::Momentum;
            if started && !continuing_momentum {
                self.clear_target_latch();
                self.wheel_target = Some(fallback_target);
                self.last_pointer = Some((cx, cy));
            } else if matches!(
                self.phase,
                WheelGesturePhase::Idle | WheelGesturePhase::Lifted { .. }
            ) && !continuing_momentum
            {
                self.clear_target_latch();
            }
        }

        let target = self
            .wheel_target
            .filter(|&id| doc.get_node(id).is_some_and(|n| n.flags.is_in_document()))
            .unwrap_or(fallback_target);
        (target, cancelable)
    }

    /// Place a default-prevented wheel event in the gesture, returning the axes its default
    /// scroll would move and clearing any latched target.
    ///
    /// Every wheel event must pass through here or [`handle_wheel`] so the gesture tracks
    /// where it starts and ends.
    pub(crate) fn track_prevented(&mut self, event: &BlitzWheelEvent) -> WheelAxes {
        let (axes, _) = self.track_event(event, true);
        self.clear_target_latch();
        axes
    }

    fn track_event(
        &mut self,
        event: &BlitzWheelEvent,
        default_prevented: bool,
    ) -> (WheelAxes, LatchMode) {
        if event.source == BlitzWheelSource::Wheel {
            if !matches!(self.phase, WheelGesturePhase::Idle) {
                self.phase = WheelGesturePhase::Idle;
                self.clear_target_latch();
            }
            let now = Instant::now();
            let (cx, cy) = (event.client_x(), event.client_y());
            if !self.is_wheel_burst_active(now, cx, cy) {
                self.clear_target_latch();
            }
            return (WheelAxes::Both, LatchMode::Wheel);
        }

        let started = event.phase == BlitzWheelPhase::Started;
        let recent = match self.phase {
            WheelGesturePhase::Lifted { recent }
                if started && event.source == BlitzWheelSource::Momentum =>
            {
                recent
            }
            _ if started => {
                self.clear_scroll_latch();
                (0.0, 0.0)
            }
            WheelGesturePhase::Active { recent } => recent,
            WheelGesturePhase::Idle | WheelGesturePhase::Lifted { .. } => {
                self.phase = WheelGesturePhase::Idle;
                self.clear_target_latch();
                return (WheelAxes::Both, LatchMode::Ungestured);
            }
        };

        let (x, y) = event.delta.to_pixels(WHEEL_LINE_PX);
        let x = recent.0 * Self::DECAY + x.abs();
        let y = recent.1 * Self::DECAY + y.abs();
        let axes = if y >= x * Self::LOCK_RATIO {
            WheelAxes::Vertical
        } else if x >= y * Self::LOCK_RATIO {
            WheelAxes::Horizontal
        } else {
            WheelAxes::Both
        };

        let mut ends = false;
        self.phase = match event.phase {
            BlitzWheelPhase::Ended if event.source == BlitzWheelSource::Finger => {
                WheelGesturePhase::Lifted { recent: (x, y) }
            }
            BlitzWheelPhase::Ended | BlitzWheelPhase::Cancelled => {
                ends = true;
                WheelGesturePhase::Idle
            }
            BlitzWheelPhase::Started | BlitzWheelPhase::Moved => {
                WheelGesturePhase::Active { recent: (x, y) }
            }
        };
        if default_prevented && ends {
            self.clear_target_latch();
        }
        (axes, LatchMode::Gesture { ends })
    }
}

/// How far (in CSS pixels) a wheel event scrolls for each line of its delta.
const WHEEL_LINE_PX: f64 = 20.0;

fn resolve_axis_target(
    doc: &BaseDocument,
    slot: &mut Option<ScrollTarget>,
    hover: Option<NodeId>,
    delta: Point<f64>,
) -> ScrollTarget {
    if let Some(target) = *slot
        && doc.is_valid_scroll_target(target)
    {
        return target;
    }
    let target = doc.resolve_wheel_scroll_target(hover, delta);
    *slot = Some(target);
    target
}

fn scroll_latched(
    doc: &mut BaseDocument,
    event: &BlitzWheelEvent,
    scroll_x: f64,
    scroll_y: f64,
    is_wheel: bool,
) -> bool {
    let hover = doc.get_hover_node_id();
    let cx = event.client_x();
    let cy = event.client_y();

    let mut gesture = doc.wheel_gesture;
    if is_wheel {
        let now = Instant::now();
        let (_, lx, ly) = gesture.wheel_latch.unwrap_or((now, cx, cy));
        gesture.wheel_latch = Some((now, lx, ly));
    }
    gesture.last_pointer.get_or_insert((cx, cy));

    let target_x = (scroll_x != 0.0).then(|| {
        resolve_axis_target(
            doc,
            &mut gesture.latched_x,
            hover,
            Point {
                x: -scroll_x,
                y: 0.0,
            },
        )
    });
    let target_y = (scroll_y != 0.0).then(|| {
        resolve_axis_target(
            doc,
            &mut gesture.latched_y,
            hover,
            Point {
                x: 0.0,
                y: -scroll_y,
            },
        )
    });
    doc.wheel_gesture = gesture;

    match (target_x, target_y) {
        (Some(tx), Some(ty)) if tx == ty => {
            doc.scroll_target_by(tx, scroll_x, scroll_y, ScrollOverflow::Clamp)
        }
        (tx, ty) => {
            let mut changed = false;
            if let Some(tx) = tx {
                changed |= doc.scroll_target_by(tx, scroll_x, 0.0, ScrollOverflow::Clamp);
            }
            if let Some(ty) = ty {
                changed |= doc.scroll_target_by(ty, 0.0, scroll_y, ScrollOverflow::Clamp);
            }
            changed
        }
    }
}

pub(crate) fn handle_wheel(doc: &mut BaseDocument, _: NodeId, event: BlitzWheelEvent) {
    let (raw_x, raw_y) = event.delta.to_pixels(WHEEL_LINE_PX);
    let (axes, mode) = doc.wheel_gesture.track_event(&event, false);
    let (scroll_x, scroll_y) = match axes {
        WheelAxes::Both => (raw_x, raw_y),
        WheelAxes::Horizontal => (raw_x, 0.0),
        WheelAxes::Vertical => (0.0, raw_y),
    };

    if scroll_x == 0.0 && scroll_y == 0.0 {
        if matches!(mode, LatchMode::Gesture { ends: true }) {
            doc.wheel_gesture.clear_target_latch();
        }
        return;
    }

    let has_changed = match mode {
        LatchMode::Ungestured => doc.scroll_chain_by(doc.get_hover_node_id(), scroll_x, scroll_y),
        LatchMode::Gesture { ends } => {
            let changed = scroll_latched(doc, &event, scroll_x, scroll_y, false);
            if ends {
                doc.wheel_gesture.clear_target_latch();
            }
            changed
        }
        LatchMode::Wheel => scroll_latched(doc, &event, scroll_x, scroll_y, true),
    };

    if has_changed {
        doc.shell_provider.request_redraw();
    }
}
