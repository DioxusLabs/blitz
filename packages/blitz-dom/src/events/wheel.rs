use blitz_traits::{
    events::{BlitzWheelEvent, BlitzWheelPhase, BlitzWheelSource},
    node_id::NodeId,
};

use crate::BaseDocument;

/// The axes a wheel event's default scroll moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WheelAxes {
    #[default]
    Both,
    Horizontal,
    Vertical,
}

/// Locks a trackpad scroll gesture to its main axis, as browsers do. Swipes are rarely
/// straight, so without a lock a vertical swipe also nudges horizontal scrollers sideways.
///
/// Each event is locked by the gesture's recent movement, as in Chromium: to an axis while
/// the movement mostly follows it, and to neither while it is diagonal. So a gesture that
/// turns unlocks and can lock again. Momentum scrolling carries on from the gesture it
/// follows. Events from devices that report no gestures scroll as they come.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum WheelGesture {
    /// No gesture is under way: events scroll both axes.
    #[default]
    Idle,
    /// A gesture is under way. `recent` is how far it has moved along each axis, counting
    /// each event's movement for less as newer events arrive.
    Active { recent: (f64, f64) },
    /// The fingers have lifted from a gesture. Momentum scrolling that follows directly
    /// carries on from its recent movement.
    Lifted { recent: (f64, f64) },
}

impl WheelGesture {
    /// An event locks to an axis when the recent movement along that axis is at least this
    /// many times the movement along the other.
    const LOCK_RATIO: f64 = 2.0;
    /// How much of the recent movement is kept each time an event arrives.
    const DECAY: f64 = 0.8;

    /// Place a wheel event in the gesture, returning the axes its default scroll moves.
    ///
    /// Every wheel event must pass through here exactly once, including those whose default
    /// scroll is prevented, or the gesture loses track of where it starts and ends.
    pub(crate) fn track(&mut self, event: &BlitzWheelEvent) -> WheelAxes {
        if event.source == BlitzWheelSource::Wheel {
            *self = Self::Idle;
            return WheelAxes::Both;
        }

        let started = event.phase == BlitzWheelPhase::Started;
        let recent = match *self {
            Self::Lifted { recent } if started && event.source == BlitzWheelSource::Momentum => {
                recent
            }
            _ if started => (0.0, 0.0),
            Self::Active { recent } => recent,
            Self::Idle | Self::Lifted { .. } => {
                *self = Self::Idle;
                return WheelAxes::Both;
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

        *self = match event.phase {
            BlitzWheelPhase::Ended if event.source == BlitzWheelSource::Finger => {
                Self::Lifted { recent: (x, y) }
            }
            BlitzWheelPhase::Ended | BlitzWheelPhase::Cancelled => Self::Idle,
            BlitzWheelPhase::Started | BlitzWheelPhase::Moved => Self::Active { recent: (x, y) },
        };
        axes
    }
}

/// How far (in CSS pixels) a wheel event scrolls for each line of its delta.
const WHEEL_LINE_PX: f64 = 20.0;

pub(crate) fn handle_wheel(doc: &mut BaseDocument, _: NodeId, event: BlitzWheelEvent) {
    let (scroll_x, scroll_y) = event.delta.to_pixels(WHEEL_LINE_PX);
    let (scroll_x, scroll_y) = match doc.wheel_gesture.track(&event) {
        WheelAxes::Both => (scroll_x, scroll_y),
        WheelAxes::Horizontal => (scroll_x, 0.0),
        WheelAxes::Vertical => (0.0, scroll_y),
    };

    let has_changed = doc.scroll_chain_by(doc.get_hover_node_id(), scroll_x, scroll_y);
    if has_changed {
        doc.shell_provider.request_redraw();
    }
}
