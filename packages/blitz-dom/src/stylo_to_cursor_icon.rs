use cursor_icon::CursorIcon;
use style::values::computed::ui::CursorKind as StyloCursorKind;

pub(crate) fn stylo_to_cursor_icon(cursor: StyloCursorKind) -> CursorIcon {
    match cursor {
        StyloCursorKind::None => todo!("set the cursor to none"),
        StyloCursorKind::Default => CursorIcon::Default,
        StyloCursorKind::Pointer => CursorIcon::Pointer,
        StyloCursorKind::ContextMenu => CursorIcon::ContextMenu,
        StyloCursorKind::Help => CursorIcon::Help,
        StyloCursorKind::Progress => CursorIcon::Progress,
        StyloCursorKind::Wait => CursorIcon::Wait,
        StyloCursorKind::Cell => CursorIcon::Cell,
        StyloCursorKind::Crosshair => CursorIcon::Crosshair,
        StyloCursorKind::Text => CursorIcon::Text,
        StyloCursorKind::VerticalText => CursorIcon::VerticalText,
        StyloCursorKind::Alias => CursorIcon::Alias,
        StyloCursorKind::Copy => CursorIcon::Copy,
        StyloCursorKind::Move => CursorIcon::Move,
        StyloCursorKind::NoDrop => CursorIcon::NoDrop,
        StyloCursorKind::NotAllowed => CursorIcon::NotAllowed,
        StyloCursorKind::Grab => CursorIcon::Grab,
        StyloCursorKind::Grabbing => CursorIcon::Grabbing,
        StyloCursorKind::EResize => CursorIcon::EResize,
        StyloCursorKind::NResize => CursorIcon::NResize,
        StyloCursorKind::NeResize => CursorIcon::NeResize,
        StyloCursorKind::NwResize => CursorIcon::NwResize,
        StyloCursorKind::SResize => CursorIcon::SResize,
        StyloCursorKind::SeResize => CursorIcon::SeResize,
        StyloCursorKind::SwResize => CursorIcon::SwResize,
        StyloCursorKind::WResize => CursorIcon::WResize,
        StyloCursorKind::EwResize => CursorIcon::EwResize,
        StyloCursorKind::NsResize => CursorIcon::NsResize,
        StyloCursorKind::NeswResize => CursorIcon::NeswResize,
        StyloCursorKind::NwseResize => CursorIcon::NwseResize,
        StyloCursorKind::ColResize => CursorIcon::ColResize,
        StyloCursorKind::RowResize => CursorIcon::RowResize,
        StyloCursorKind::AllScroll => CursorIcon::AllScroll,
        StyloCursorKind::ZoomIn => CursorIcon::ZoomIn,
        StyloCursorKind::ZoomOut => CursorIcon::ZoomOut,
        StyloCursorKind::Auto => {
            // todo: we should be the ones determining this based on the UA?
            // https://developer.mozilla.org/en-US/docs/Web/CSS/cursor

            CursorIcon::Default
        }
    }
}
