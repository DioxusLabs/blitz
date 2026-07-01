use cursor_icon::CursorIcon;
use style::values::computed::ui::CursorKind as StyloCursorKind;

pub(crate) fn stylo_to_cursor_icon(cursor: StyloCursorKind) -> Option<CursorIcon> {
    match cursor {
        // Auto needs special handling so this function should only really be called with
        // non-Auto values. But better to return a default than panic if it is.
        StyloCursorKind::Auto => {
            #[cfg(feature = "tracing")]
            tracing::warn!("stylo_to_cursor_icon was called with CursorKind::Auto");
            Some(CursorIcon::Default)
        }
        StyloCursorKind::None => None,
        StyloCursorKind::Default => Some(CursorIcon::Default),
        StyloCursorKind::Pointer => Some(CursorIcon::Pointer),
        StyloCursorKind::ContextMenu => Some(CursorIcon::ContextMenu),
        StyloCursorKind::Help => Some(CursorIcon::Help),
        StyloCursorKind::Progress => Some(CursorIcon::Progress),
        StyloCursorKind::Wait => Some(CursorIcon::Wait),
        StyloCursorKind::Cell => Some(CursorIcon::Cell),
        StyloCursorKind::Crosshair => Some(CursorIcon::Crosshair),
        StyloCursorKind::Text => Some(CursorIcon::Text),
        StyloCursorKind::VerticalText => Some(CursorIcon::VerticalText),
        StyloCursorKind::Alias => Some(CursorIcon::Alias),
        StyloCursorKind::Copy => Some(CursorIcon::Copy),
        StyloCursorKind::Move => Some(CursorIcon::Move),
        StyloCursorKind::NoDrop => Some(CursorIcon::NoDrop),
        StyloCursorKind::NotAllowed => Some(CursorIcon::NotAllowed),
        StyloCursorKind::Grab => Some(CursorIcon::Grab),
        StyloCursorKind::Grabbing => Some(CursorIcon::Grabbing),
        StyloCursorKind::EResize => Some(CursorIcon::EResize),
        StyloCursorKind::NResize => Some(CursorIcon::NResize),
        StyloCursorKind::NeResize => Some(CursorIcon::NeResize),
        StyloCursorKind::NwResize => Some(CursorIcon::NwResize),
        StyloCursorKind::SResize => Some(CursorIcon::SResize),
        StyloCursorKind::SeResize => Some(CursorIcon::SeResize),
        StyloCursorKind::SwResize => Some(CursorIcon::SwResize),
        StyloCursorKind::WResize => Some(CursorIcon::WResize),
        StyloCursorKind::EwResize => Some(CursorIcon::EwResize),
        StyloCursorKind::NsResize => Some(CursorIcon::NsResize),
        StyloCursorKind::NeswResize => Some(CursorIcon::NeswResize),
        StyloCursorKind::NwseResize => Some(CursorIcon::NwseResize),
        StyloCursorKind::ColResize => Some(CursorIcon::ColResize),
        StyloCursorKind::RowResize => Some(CursorIcon::RowResize),
        StyloCursorKind::AllScroll => Some(CursorIcon::AllScroll),
        StyloCursorKind::ZoomIn => Some(CursorIcon::ZoomIn),
        StyloCursorKind::ZoomOut => Some(CursorIcon::ZoomOut),
    }
}
