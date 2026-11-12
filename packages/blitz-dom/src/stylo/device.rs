//! Management of the Stylo [`Device`]: constructing it from shell state and
//! coalescing changes to it (viewport resizes, zoom, color-scheme and media
//! type changes) so that the stylist device is updated at most once per
//! [`resolve`](crate::BaseDocument::resolve), no matter how many changes
//! occurred since the last resolve.

use std::sync::{Arc, Mutex};

use bitflags::bitflags;
use blitz_traits::shell::{ColorScheme, Viewport};
use parley::FontContext;
use selectors::matching::QuirksMode;
use style::device::Device;
use style::device::servo::MediaData;
use style::media_queries::MediaType;
use style::properties::ComputedValues;
use style::properties::style_structs::Font;
use style::queries::values::PrefersColorScheme;
use style::servo::media_features::PointerCapabilities;

use crate::font_metrics::BlitzFontMetricsProvider;

bitflags! {
    /// The set of changes to the [`Device`] that have accumulated since the
    /// stylist device was last rebuilt. Only the *latest* values matter (they
    /// live on [`BaseDocument`](crate::BaseDocument)); these flags record
    /// which kinds of change occurred so that the flush can do the right
    /// amount of invalidation work.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub(crate) struct DeviceChanges: u8 {
        /// The CSS pixel viewport size changed (window resize, zoom, or hidpi
        /// scale change).
        const VIEWPORT_SIZE = 0b0000_0001;
        /// The total scale factor (hidpi scale * zoom) changed. Requires
        /// invalidating cached inline layouts (text is shaped at a specific
        /// scale), recomputing every node's transform and scrollable overflow
        /// (kept in device pixels) and a redraw.
        const SCALE = 0b0000_0010;
        /// The preferred color scheme changed.
        const COLOR_SCHEME = 0b0000_0100;
        /// The media type (e.g. screen/print) changed.
        const MEDIA_TYPE = 0b0000_1000;
    }
}

impl DeviceChanges {
    /// Compute the [`DeviceChanges`] implied by moving from viewport `old` to
    /// viewport `new`.
    pub(crate) fn from_viewports(old: &Viewport, new: &Viewport) -> Self {
        let mut changes = Self::empty();
        if old.logical_size() != new.logical_size() {
            changes |= Self::VIEWPORT_SIZE;
        }
        if old.scale_f64() != new.scale_f64() {
            changes |= Self::SCALE;
        }
        if old.color_scheme != new.color_scheme {
            changes |= Self::COLOR_SCHEME;
        }
        changes
    }
}

pub(crate) fn make_device(
    viewport: &Viewport,
    media_type: MediaType,
    font_ctx: Arc<Mutex<FontContext>>,
) -> Device {
    Device::new(
        ComputedValues::initial_values_with_font_override(Font::initial_values()),
        Box::new(BlitzFontMetricsProvider { font_ctx }),
        "Submit Query".into(),
        make_media_data(viewport, media_type),
    )
}

/// The [`MediaData`] (the media-query-visible state of the [`Device`]) for the given viewport
/// and media type.
pub(crate) fn make_media_data(viewport: &Viewport, media_type: MediaType) -> MediaData {
    let (width, height) = viewport.logical_size();
    let viewport_size = euclid::Size2D::new(width, height);
    let device_pixel_ratio = euclid::Scale::new(viewport.scale());
    MediaData {
        media_type,
        viewport_size,
        device_size: viewport_size * device_pixel_ratio,
        device_pixel_ratio,
        quirks_mode: QuirksMode::NoQuirks,
        prefers_color_scheme: match viewport.color_scheme {
            ColorScheme::Light => PrefersColorScheme::Light,
            ColorScheme::Dark => PrefersColorScheme::Dark,
        },
        primary_pointer_capabilities: PointerCapabilities::default(),
        all_pointer_capabilities: PointerCapabilities::default(),
    }
}
