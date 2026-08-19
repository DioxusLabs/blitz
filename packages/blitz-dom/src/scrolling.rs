//! Scrolling: user-initiated (interactive) and programmatic scrolling of nodes and the
//! viewport, and the scroll animations (smooth scrolls and flings) which drive them.

use blitz_traits::events::{BlitzScrollEvent, DomEvent, DomEventData};
use blitz_traits::node_id::NodeId;
use style::values::computed::Overflow;
use web_time::{SystemTime, UNIX_EPOCH};

use crate::BaseDocument;
use crate::util::Point;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollBehavior {
    #[default]
    Auto,
    Instant,
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollLogicalPosition {
    Start,
    Center,
    End,
    Nearest,
}

/// What a scroll applies to.
///
/// Per the CSS overflow propagation rules the root element has no scrolling mechanism of its
/// own (its overflow is applied to the viewport), so [`ScrollTarget::Node`] holding the root
/// element is equivalent to [`ScrollTarget::Viewport`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollTarget {
    Node(NodeId),
    Viewport,
}

/// How far to scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ScrollAmount {
    /// An absolute scroll offset.
    To(Point<f64>),
    /// A delta to apply to the current scroll offset.
    By(Point<f64>),
}

/// What to do with scroll which the target cannot consume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollOverflow {
    /// Discard it: the scroll affects exactly one scroller.
    Clamp,
    /// Transfer it to the next scroller in the scroll chain (the parent node, and eventually
    /// the viewport).
    Chain,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollRequest {
    pub(crate) target: ScrollTarget,
    pub(crate) amount: ScrollAmount,
    pub(crate) overflow: ScrollOverflow,
    pub(crate) behavior: ScrollBehavior,
    /// Whether to abort a smooth scroll in progress. Set for user-initiated scrolls, so that
    /// an animation does not fight the user's input for the rest of its duration.
    pub(crate) interrupt_animation: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FlingState {
    pub(crate) target: NodeId,
    pub(crate) last_seen_time: f64,
    pub(crate) x_velocity: f64,
    pub(crate) y_velocity: f64,
}

/// State driving a smooth (animated) scroll towards a target offset. Used for
/// fragment navigation (`#anchor` links) and programmatic smooth scrolling.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScrollToState {
    /// What is being scrolled.
    pub(crate) target: ScrollTarget,
    /// The scroll offset at the start of the animation.
    pub(crate) start: Point<f64>,
    /// The scroll offset to animate towards.
    pub(crate) end: Point<f64>,
    /// Time (in milliseconds since the Unix epoch) at which the animation started.
    pub(crate) start_time: f64,
    /// Total duration of the animation in milliseconds.
    pub(crate) duration: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ScrollAnimationState {
    None,
    Fling(FlingState),
    /// A smooth scroll of the viewport towards a target offset.
    ScrollTo(ScrollToState),
}

/// Cubic ease-in-out easing function, mapping a normalised time `t` in `[0, 1]`
/// to an eased progress value in `[0, 1]`. Used to give smooth scrolls a natural
/// acceleration/deceleration curve.
fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let f = 2.0 * t - 2.0;
        1.0 + (f * f * f) / 2.0
    }
}

impl BaseDocument {
    /// Apply a scroll to the document, returning whether anything moved.
    ///
    /// This is the single scrolling primitive: user-initiated scrolls
    /// ([`BaseDocument::scroll_chain_by`]) and programmatic scrolls
    /// ([`BaseDocument::scroll_to`]) differ only in the [`ScrollRequest`] they build.
    pub(crate) fn scroll(
        &mut self,
        request: ScrollRequest,
        dispatch_event: &mut dyn FnMut(DomEvent),
    ) -> bool {
        if request.interrupt_animation
            && matches!(self.scroll_animation, ScrollAnimationState::ScrollTo(_))
        {
            self.scroll_animation = ScrollAnimationState::None;
        }

        let target = self.canonical_scroll_target(request.target);

        // Text inputs and sub-documents scroll their own content rather than an overflow
        // scrollport, so they only take part in the chained (user-initiated) path.
        if let (ScrollTarget::Node(node_id), ScrollAmount::By(delta), ScrollOverflow::Chain) =
            (target, request.amount, request.overflow)
        {
            if let Some(has_changed) =
                self.scroll_inner_content_by(node_id, delta, request, dispatch_event)
            {
                return has_changed;
            }
        }

        let (current, max) = self.scroll_state(target);
        let unclamped = match request.amount {
            ScrollAmount::To(to) => to,
            ScrollAmount::By(by) => Point {
                x: current.x + by.x,
                y: current.y + by.y,
            },
        };
        let end = Point {
            x: unclamped.x.clamp(0.0, max.x),
            y: unclamped.y.clamp(0.0, max.y),
        };

        if self.should_scroll_smoothly(target, request.behavior) {
            self.start_scroll_animation(target, end);
            return end != current;
        }

        let has_changed = self.write_scroll_offset(target, end, dispatch_event);

        // Transfer the delta the target could not consume to the next scroller in the chain.
        if request.overflow == ScrollOverflow::Chain {
            let remainder = Point {
                x: unclamped.x - end.x,
                y: unclamped.y - end.y,
            };
            if remainder != Point::ZERO {
                if let Some(next) = self.next_scroller_in_chain(target) {
                    let request = ScrollRequest {
                        target: next,
                        amount: ScrollAmount::By(remainder),
                        ..request
                    };
                    return has_changed | self.scroll(request, dispatch_event);
                }
            }
        }

        has_changed
    }

    /// Scroll a node which scrolls content of its own rather than an overflow scrollport
    /// (a text input or a sub-document), returning `None` if the node is not such a node.
    fn scroll_inner_content_by(
        &mut self,
        node_id: NodeId,
        delta: Point<f64>,
        request: ScrollRequest,
        dispatch_event: &mut dyn FnMut(DomEvent),
    ) -> Option<bool> {
        let node = self.nodes.get_mut(node_id)?;

        if let Some(mut sub_doc) = node.subdoc_mut().map(|doc| doc.inner_mut()) {
            let target = sub_doc
                .get_hover_node_id()
                .map_or(ScrollTarget::Viewport, ScrollTarget::Node);
            // TODO: propagate the remaining scroll to the outer document
            let request = ScrollRequest {
                target,
                amount: ScrollAmount::By(delta),
                ..request
            };
            return Some(sub_doc.scroll(request, dispatch_event));
        }

        // Single-line inputs scroll their text horizontally, multi-line inputs vertically.
        node.element_data()?.text_input_data()?;
        let content_box_width = node.final_layout().content_box_width();
        let content_box_height = node.final_layout().content_box_height();
        let input = node
            .element_data_mut()
            .and_then(|el| el.text_input_data_mut())
            .unwrap();

        // `TextInputData::scroll_by` takes (and returns the unconsumed part of) a delta in
        // the opposite direction to a scroll offset.
        let mut remainder = delta;
        if input.is_multiline {
            remainder.y =
                -(input.scroll_by(-delta.y as f32, content_box_width, content_box_height) as f64);
        } else {
            remainder.x =
                -(input.scroll_by(-delta.x as f32, content_box_width, content_box_height) as f64);
        }

        let has_changed = remainder != delta;

        if remainder != Point::ZERO {
            if let Some(next) = self.next_scroller_in_chain(ScrollTarget::Node(node_id)) {
                let request = ScrollRequest {
                    target: next,
                    amount: ScrollAmount::By(remainder),
                    ..request
                };
                return Some(has_changed | self.scroll(request, dispatch_event));
            }
        }

        Some(has_changed)
    }

    /// The scroller which unconsumed scroll is transferred to: the node's parent, or the
    /// viewport once the chain runs out of nodes.
    fn next_scroller_in_chain(&self, target: ScrollTarget) -> Option<ScrollTarget> {
        match target {
            ScrollTarget::Viewport => None,
            ScrollTarget::Node(node_id) => Some(
                self.nodes
                    .get(node_id)
                    .and_then(|node| node.parent)
                    .map_or(ScrollTarget::Viewport, ScrollTarget::Node),
            ),
        }
    }

    /// Resolve a scroll target to the scroller which actually moves: the root element scrolls
    /// the viewport, per the CSS overflow propagation rules.
    fn canonical_scroll_target(&self, target: ScrollTarget) -> ScrollTarget {
        match target {
            ScrollTarget::Node(node_id)
                if self.try_root_element().is_some_and(|el| el.id == node_id) =>
            {
                ScrollTarget::Viewport
            }
            target => target,
        }
    }

    /// Write a scroll target's offset (which must already be clamped to its scrollable
    /// range), dispatching a `scroll` event and returning whether the offset changed.
    pub(crate) fn write_scroll_offset(
        &mut self,
        target: ScrollTarget,
        offset: Point<f64>,
        dispatch_event: &mut dyn FnMut(DomEvent),
    ) -> bool {
        match self.canonical_scroll_target(target) {
            ScrollTarget::Viewport => {
                let initial = self.viewport_scroll;
                self.viewport_scroll = offset;
                if offset == initial {
                    return false;
                }

                if let Some(root) = self.try_root_element() {
                    let root_id = root.id;
                    let layout = *root.final_layout();
                    let scale = self.viewport.scale() as f64;
                    let event = BlitzScrollEvent {
                        scroll_top: offset.y,
                        scroll_left: offset.x,
                        scroll_width: layout.size.width.max(layout.scrollable_overflow_rect.right)
                            as i32,
                        scroll_height: layout
                            .size
                            .height
                            .max(layout.scrollable_overflow_rect.bottom)
                            as i32,
                        client_width: (self.viewport.window_size.0 as f64 / scale) as i32,
                        client_height: (self.viewport.window_size.1 as f64 / scale) as i32,
                    };
                    dispatch_event(DomEvent::new(root_id, DomEventData::Scroll(event)));
                }

                self.shell_provider.request_redraw();
                true
            }
            ScrollTarget::Node(node_id) => {
                let Some(node) = self.nodes.get_mut(node_id) else {
                    return false;
                };

                let initial = *node.scroll_offset();
                *node.scroll_offset_mut() = offset;
                if offset == initial {
                    return false;
                }

                let layout = *node.final_layout();
                let event = BlitzScrollEvent {
                    scroll_top: offset.y,
                    scroll_left: offset.x,
                    scroll_width: layout.scroll_width() as i32,
                    scroll_height: layout.scroll_height() as i32,
                    client_width: layout.size.width as i32,
                    client_height: layout.size.height as i32,
                };
                dispatch_event(DomEvent::new(node_id, DomEventData::Scroll(event)));

                self.show_scrollbars(node_id);
                self.shell_provider.request_redraw();
                true
            }
        }
    }

    pub fn scroll_node_by<F: FnMut(DomEvent)>(
        &mut self,
        node_id: NodeId,
        x: f64,
        y: f64,
        dispatch_event: F,
    ) {
        self.scroll_node_by_has_changed(node_id, x, y, dispatch_event);
    }

    /// Scroll a node by given x and y
    /// Will bubble scrolling up to parent node once it can no longer scroll further
    /// If we're already at the root node, bubbles scrolling up to the viewport
    pub fn scroll_node_by_has_changed<F: FnMut(DomEvent)>(
        &mut self,
        node_id: NodeId,
        x: f64,
        y: f64,
        mut dispatch_event: F,
    ) -> bool {
        self.scroll(
            ScrollRequest {
                target: ScrollTarget::Node(node_id),
                // A user-facing scroll delta moves the content, i.e. the opposite direction
                // to the scroll offset.
                amount: ScrollAmount::By(Point { x: -x, y: -y }),
                overflow: ScrollOverflow::Chain,
                behavior: ScrollBehavior::Instant,
                interrupt_animation: false,
            },
            &mut dispatch_event,
        )
    }

    pub fn scroll_viewport_by(&mut self, x: f64, y: f64) {
        self.scroll_viewport_by_has_changed(x, y);
    }

    /// Scroll the viewport by the given values
    pub fn scroll_viewport_by_has_changed(&mut self, x: f64, y: f64) -> bool {
        self.scroll(
            ScrollRequest {
                target: ScrollTarget::Viewport,
                amount: ScrollAmount::By(Point { x: -x, y: -y }),
                overflow: ScrollOverflow::Clamp,
                behavior: ScrollBehavior::Instant,
                interrupt_animation: false,
            },
            &mut |_| {},
        )
    }

    pub(crate) fn scroll_chain_by(
        &mut self,
        anchor_node_id: Option<NodeId>,
        scroll_x: f64,
        scroll_y: f64,
        dispatch_event: &mut dyn FnMut(DomEvent),
    ) -> bool {
        self.scroll(
            ScrollRequest {
                target: anchor_node_id.map_or(ScrollTarget::Viewport, ScrollTarget::Node),
                amount: ScrollAmount::By(Point {
                    x: -scroll_x,
                    y: -scroll_y,
                }),
                overflow: ScrollOverflow::Chain,
                behavior: ScrollBehavior::Instant,
                // A user-initiated scroll aborts any smooth scroll in progress, so that the
                // two do not fight over the scroll offset for the rest of the animation.
                interrupt_animation: true,
            },
            dispatch_event,
        )
    }

    /// Duration (in milliseconds) of an animated scroll.
    const SMOOTH_SCROLL_DURATION_MS: f64 = 300.0;

    /// Returns the current scroll offset and the maximum scroll offset (the minimum is
    /// always `0`) for the given scroll target.
    pub(crate) fn scroll_state(&self, target: ScrollTarget) -> (Point<f64>, Point<f64>) {
        match self.canonical_scroll_target(target) {
            ScrollTarget::Viewport => {
                // The viewport scrolls the root element's scrollable overflow, which includes
                // both the root element itself and any content which overflows it (e.g. when
                // the root element has a fixed height but its content is taller). A document
                // without a root element has no scrollable content.
                let (content_width, content_height) = match self.try_root_element() {
                    Some(root) => {
                        let layout = root.final_layout();
                        (
                            layout.size.width.max(layout.scrollable_overflow_rect.right) as f64,
                            layout
                                .size
                                .height
                                .max(layout.scrollable_overflow_rect.bottom)
                                as f64,
                        )
                    }
                    None => (0.0, 0.0),
                };
                let scale = self.viewport.scale() as f64;
                let window_width = self.viewport.window_size.0 as f64 / scale;
                let window_height = self.viewport.window_size.1 as f64 / scale;
                let max = Point {
                    x: (content_width - window_width).max(0.0),
                    y: (content_height - window_height).max(0.0),
                };
                (self.viewport_scroll, max)
            }
            ScrollTarget::Node(node_id) => {
                let Some(node) = self.nodes.get(node_id) else {
                    return (Point::ZERO, Point::ZERO);
                };

                // An axis with a non-scrolling overflow value has no scrollable range, even
                // when its content overflows.
                let (can_x_scroll, can_y_scroll) = node
                    .primary_styles()
                    .map(|styles| {
                        (
                            matches!(styles.clone_overflow_x(), Overflow::Scroll | Overflow::Auto),
                            matches!(styles.clone_overflow_y(), Overflow::Scroll | Overflow::Auto),
                        )
                    })
                    .unwrap_or((false, false));
                let max = Point {
                    x: match can_x_scroll {
                        true => node.final_layout().scroll_width() as f64,
                        false => 0.0,
                    },
                    y: match can_y_scroll {
                        true => node.final_layout().scroll_height() as f64,
                        false => 0.0,
                    },
                };
                (*node.scroll_offset(), max)
            }
        }
    }

    /// Start a smooth (animated) scroll towards the given absolute scroll offset. The
    /// animation is advanced each frame in [`BaseDocument::resolve_scroll_animation`].
    fn start_scroll_animation(&mut self, target: ScrollTarget, end: Point<f64>) {
        let start = self.scroll_state(target).0;

        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64 as f64;

        self.scroll_animation = ScrollAnimationState::ScrollTo(ScrollToState {
            target,
            start,
            end,
            start_time,
            duration: Self::SMOOTH_SCROLL_DURATION_MS,
        });

        // Ensure the frame loop runs so the animation is driven to completion.
        self.shell_provider.request_redraw();
    }

    fn should_scroll_smoothly(&self, target: ScrollTarget, behavior: ScrollBehavior) -> bool {
        match behavior {
            ScrollBehavior::Auto => {
                let styled_node = match target {
                    ScrollTarget::Node(node_id) => self.nodes.get(node_id),
                    ScrollTarget::Viewport => self.try_root_element(),
                };
                styled_node.is_some_and(|node| {
                    node.primary_styles().is_some_and(|style| {
                        style.clone_scroll_behavior()
                            == style::computed_values::scroll_behavior::T::Smooth
                    })
                })
            }
            ScrollBehavior::Instant => false,
            ScrollBehavior::Smooth => true,
        }
    }

    /// Scroll an element to the given absolute scroll offset in CSS pixels.
    ///
    /// Unlike a user-initiated scroll, a programmatic scroll targets exactly one scroller:
    /// scroll the element cannot consume is discarded rather than transferred to an ancestor.
    pub fn scroll_to(&mut self, node_id: NodeId, x: f64, y: f64, behavior: ScrollBehavior) {
        self.scroll_programmatically(node_id, ScrollAmount::To(Point { x, y }), behavior);
    }

    /// Scroll an element by the given relative offset in CSS pixels.
    pub fn scroll_by(&mut self, node_id: NodeId, x: f64, y: f64, behavior: ScrollBehavior) {
        self.scroll_programmatically(node_id, ScrollAmount::By(Point { x, y }), behavior);
    }

    fn scroll_programmatically(
        &mut self,
        node_id: NodeId,
        amount: ScrollAmount,
        behavior: ScrollBehavior,
    ) {
        if self.nodes.get(node_id).is_none() {
            return;
        }

        // TODO: dispatch `scroll` events for programmatic scrolls.
        self.scroll(
            ScrollRequest {
                target: ScrollTarget::Node(node_id),
                amount,
                overflow: ScrollOverflow::Clamp,
                behavior,
                interrupt_animation: true,
            },
            &mut |_| {},
        );
    }

    fn aligned_scroll_offset(
        current: f64,
        viewport_size: f64,
        target_start: f64,
        target_size: f64,
        position: ScrollLogicalPosition,
    ) -> f64 {
        let target_end = target_start + target_size;
        match position {
            ScrollLogicalPosition::Start => target_start,
            ScrollLogicalPosition::Center => target_start - (viewport_size - target_size) / 2.0,
            ScrollLogicalPosition::End => target_end - viewport_size,
            ScrollLogicalPosition::Nearest => {
                let viewport_end = current + viewport_size;
                if (target_start >= current && target_end <= viewport_end)
                    || (target_start <= current && target_end >= viewport_end)
                {
                    current
                } else {
                    let start_offset = target_start;
                    let end_offset = target_end - viewport_size;
                    if (start_offset - current).abs() < (end_offset - current).abs() {
                        start_offset
                    } else {
                        end_offset
                    }
                }
            }
        }
    }

    /// Scroll the viewport so that the given element has the requested alignment in each axis.
    pub fn scroll_into_view(
        &mut self,
        node_id: NodeId,
        behavior: ScrollBehavior,
        vertical: ScrollLogicalPosition,
        horizontal: ScrollLogicalPosition,
    ) {
        let Some(node) = self.nodes.get(node_id) else {
            return;
        };
        let target =
            node.absolute_position(node.scroll_offset().x as f32, node.scroll_offset().y as f32);
        let target_size = node.final_layout().size;
        let Some(root_id) = self.try_root_element().map(|root| root.id) else {
            return;
        };
        let scale = self.viewport.scale() as f64;
        let viewport_width = self.viewport.window_size.0 as f64 / scale;
        let viewport_height = self.viewport.window_size.1 as f64 / scale;
        let x = Self::aligned_scroll_offset(
            self.viewport_scroll.x,
            viewport_width,
            target.x as f64,
            target_size.width as f64,
            horizontal,
        );
        let y = Self::aligned_scroll_offset(
            self.viewport_scroll.y,
            viewport_height,
            target.y as f64,
            target_size.height as f64,
            vertical,
        );
        self.scroll_to(root_id, x, y, behavior);
    }

    /// Resolve a URL fragment (the `#...` part of a URL) to a scroll target.
    ///
    /// Returns `None` if the fragment matches no element and is not a top-of-document
    /// fragment. Otherwise returns `Some(target)`, where `target` is `Some(node_id)` for
    /// the element to scroll to, or `None` to scroll to the top of the document (matching
    /// browser behaviour for empty and `top` fragments).
    fn resolve_fragment_scroll_target(&self, fragment: &str) -> Option<Option<NodeId>> {
        // Fragments are percent-encoded in URLs (e.g. `%20`); decode before matching.
        let decoded = percent_encoding::percent_decode_str(fragment)
            .decode_utf8_lossy()
            .into_owned();

        if !decoded.is_empty() {
            if let Some(node_id) = self.get_fragment_target(&decoded) {
                return Some(Some(node_id));
            }
        }

        // An empty fragment, or the special "top" fragment when no matching element exists,
        // scrolls to the top of the document.
        if decoded.is_empty() || decoded.eq_ignore_ascii_case("top") {
            return Some(None);
        }

        None
    }

    fn scroll_to_fragment_with_behavior(
        &mut self,
        fragment: &str,
        behavior: ScrollBehavior,
    ) -> bool {
        match self.resolve_fragment_scroll_target(fragment) {
            Some(Some(node_id)) => {
                self.scroll_into_view(
                    node_id,
                    behavior,
                    ScrollLogicalPosition::Start,
                    ScrollLogicalPosition::Nearest,
                );
                true
            }
            Some(None) => {
                let root_id = self.root_element().id;
                self.scroll_to(root_id, 0.0, 0.0, behavior);
                true
            }
            None => false,
        }
    }

    /// Scroll to the element targeted by the given URL fragment (the `#...` part of a URL).
    ///
    /// An empty fragment (or a `top` fragment that matches no element) scrolls to the top
    /// of the document, matching browser behaviour. Returns `true` if a scroll target was
    /// found.
    pub fn scroll_to_fragment(&mut self, fragment: &str) -> bool {
        self.scroll_to_fragment_with_behavior(fragment, ScrollBehavior::Auto)
    }

    /// Like [`BaseDocument::scroll_to_fragment`], but animates the viewport towards the
    /// target instead of jumping instantly. Returns `true` if a scroll target was found.
    pub fn scroll_to_fragment_smooth(&mut self, fragment: &str) -> bool {
        self.scroll_to_fragment_with_behavior(fragment, ScrollBehavior::Smooth)
    }

    pub fn resolve_scroll_animation(&mut self) {
        match &mut self.scroll_animation {
            ScrollAnimationState::Fling(fling_state) => {
                let time_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64 as f64;

                let time_diff_ms = time_ms - fling_state.last_seen_time;

                // 0.95 @ 60fps normalized to actual frame times
                let deceleration = 1.0 - ((0.05 / 16.66666) * time_diff_ms);

                fling_state.x_velocity *= deceleration;
                fling_state.y_velocity *= deceleration;
                fling_state.last_seen_time = time_ms;
                let fling_state = fling_state.clone();

                let dx = fling_state.x_velocity * time_diff_ms;
                let dy = fling_state.y_velocity * time_diff_ms;

                self.scroll_chain_by(Some(fling_state.target), dx, dy, &mut |_| {});
                if fling_state.x_velocity.abs() < 0.1 && fling_state.y_velocity.abs() < 0.1 {
                    self.scroll_animation = ScrollAnimationState::None;
                }
            }
            ScrollAnimationState::ScrollTo(scroll_to) => {
                let scroll_to = scroll_to.clone();
                let time_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64 as f64;

                // Normalised progress through the animation, clamped to [0, 1].
                let progress = if scroll_to.duration <= 0.0 {
                    1.0
                } else {
                    ((time_ms - scroll_to.start_time) / scroll_to.duration).clamp(0.0, 1.0)
                };
                let eased = ease_in_out_cubic(progress);

                // Interpolate the target offset and move to it.
                let target = Point {
                    x: scroll_to.start.x + (scroll_to.end.x - scroll_to.start.x) * eased,
                    y: scroll_to.start.y + (scroll_to.end.y - scroll_to.start.y) * eased,
                };
                // TODO: dispatch `scroll` events for programmatic scrolls.
                self.write_scroll_offset(scroll_to.target, target, &mut |_| {});

                if progress >= 1.0 {
                    self.scroll_animation = ScrollAnimationState::None;
                }
            }
            ScrollAnimationState::None => {
                // Do nothing
            }
        }
    }
}
