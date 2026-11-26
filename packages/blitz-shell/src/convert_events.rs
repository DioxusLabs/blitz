use blitz_traits::events::{
    BlitzImeEvent, BlitzKeyEvent, BlitzPointerId, KeyState, PointerDetails,
};
use blitz_traits::shell::ColorScheme;
use keyboard_types::{Code, Key, Modifiers, NamedKey};
use winit::event::KeyEvent as WinitKeyEvent;
use winit::event::{ButtonSource, ElementState};
use winit::event::{FingerId, Ime, PointerKind, PointerSource};
use winit::keyboard::Key as WinitKey;
use winit::keyboard::ModifiersState as WinitModifiers;
use winit::keyboard::PhysicalKey as WinitPhysicalKey;
use winit::window::Theme;

pub(crate) fn theme_to_color_scheme(theme: Theme) -> ColorScheme {
    match theme {
        Theme::Light => ColorScheme::Light,
        Theme::Dark => ColorScheme::Dark,
    }
}

pub(crate) fn color_scheme_to_theme(scheme: ColorScheme) -> Theme {
    match scheme {
        ColorScheme::Light => Theme::Light,
        ColorScheme::Dark => Theme::Dark,
    }
}

pub(crate) fn winit_ime_to_blitz(event: Ime) -> BlitzImeEvent {
    match event {
        Ime::Enabled => BlitzImeEvent::Enabled,
        Ime::Disabled => BlitzImeEvent::Disabled,
        Ime::Preedit(text, cursor) => BlitzImeEvent::Preedit(text, cursor),
        Ime::Commit(text) => BlitzImeEvent::Commit(text),
        Ime::DeleteSurrounding {
            before_bytes,
            after_bytes,
        } => BlitzImeEvent::DeleteSurrounding {
            before_bytes,
            after_bytes,
        },
    }
}

pub(crate) fn winit_key_event_to_blitz(
    event: &WinitKeyEvent,
    mods: WinitModifiers,
) -> BlitzKeyEvent {
    BlitzKeyEvent {
        key: winit_key_to_kbt_key(&event.logical_key),
        code: winit_physical_key_to_kbt_code(&event.physical_key),
        modifiers: winit_modifiers_to_kbt_modifiers(mods),
        location: event.location,
        is_auto_repeating: event.repeat,
        is_composing: false,
        state: match event.state {
            ElementState::Pressed => KeyState::Pressed,
            ElementState::Released => KeyState::Released,
        },
        text: event.text.clone(),
    }
}

/// Blitz's own IDs for the fingers winit currently reports.
///
/// winit's `FingerId`s are platform values (on iOS a `UITouch` pointer, on
/// Windows/X11 a system counter, on Android/Wayland a reused slot), so the
/// shell assigns each finger a fresh ID from a counter when it first appears
/// and forgets it when the finger leaves. Only a handful of fingers are ever
/// active, so a linear search is enough.
#[derive(Default)]
pub(crate) struct PointerIds {
    active: Vec<(FingerId, BlitzPointerId)>,
    next_index: u64,
}

impl PointerIds {
    pub(crate) fn get_or_assign(&mut self, finger_id: FingerId) -> BlitzPointerId {
        if let Some((_, id)) = self.active.iter().find(|(f, _)| *f == finger_id) {
            return *id;
        }
        let id = BlitzPointerId::finger(self.next_index);
        self.next_index = (self.next_index + 1) % BlitzPointerId::FINGER_INDEX_COUNT;
        self.active.push((finger_id, id));
        id
    }

    pub(crate) fn remove(&mut self, finger_id: FingerId) {
        self.active.retain(|(f, _)| *f != finger_id);
    }
}

pub(crate) fn pointer_source_to_blitz(
    source: &PointerSource,
    pointer_ids: &mut PointerIds,
) -> BlitzPointerId {
    match source {
        PointerSource::Mouse => BlitzPointerId::Mouse,
        PointerSource::Touch { finger_id, .. } => pointer_ids.get_or_assign(*finger_id),

        // TODO: TabletTool and Unknown events
        PointerSource::TabletTool { .. } => BlitzPointerId::Pen,
        PointerSource::Unknown => BlitzPointerId::Mouse,
        _ => {
            #[cfg(feature = "tracing")]
            tracing::error!("unsupported PointerSource variant: {source:?}");
            BlitzPointerId::Mouse
        }
    }
}

pub(crate) fn pointer_kind_to_blitz(
    kind: &PointerKind,
    pointer_ids: &mut PointerIds,
) -> BlitzPointerId {
    match kind {
        PointerKind::Mouse => BlitzPointerId::Mouse,
        PointerKind::Touch(finger_id) => pointer_ids.get_or_assign(*finger_id),

        // TODO: TabletTool and Unknown events
        PointerKind::TabletTool(_) => BlitzPointerId::Pen,
        PointerKind::Unknown => BlitzPointerId::Mouse,
        _ => {
            #[cfg(feature = "tracing")]
            tracing::error!("unsupported PointerKind variant: {kind:?}");
            BlitzPointerId::Mouse
        }
    }
}

pub(crate) fn button_source_to_blitz(
    source: &ButtonSource,
    pointer_ids: &mut PointerIds,
) -> BlitzPointerId {
    match source {
        ButtonSource::Mouse(_) => BlitzPointerId::Mouse,
        ButtonSource::TabletTool { .. } => BlitzPointerId::Pen,
        ButtonSource::Touch { finger_id, .. } => pointer_ids.get_or_assign(*finger_id),

        // TODO: Unknown events
        ButtonSource::Unknown(_) => BlitzPointerId::Mouse,
        _ => {
            #[cfg(feature = "tracing")]
            tracing::error!("unsupported ButtonSource variant: {source:?}");
            BlitzPointerId::Mouse
        }
    }
}

pub(crate) fn pointer_source_to_blitz_details(source: &PointerSource) -> PointerDetails {
    match source {
        PointerSource::Mouse => PointerDetails::default(),
        PointerSource::Unknown => PointerDetails::default(),
        PointerSource::Touch { force, .. } => PointerDetails {
            pressure: force.map(|force| force.normalized(None)).unwrap_or(0.0),
            ..PointerDetails::default()
        },
        PointerSource::TabletTool { data, .. } => PointerDetails {
            pressure: data
                .force
                .map(|force| force.normalized(data.angle))
                .unwrap_or(0.0),
            tangential_pressure: data.tangential_force.unwrap_or(0.0),
            tilt_x: data.tilt.map(|tilt| tilt.x).unwrap_or(0),
            tilt_y: data.tilt.map(|tilt| tilt.y).unwrap_or(0),
            twist: data.twist.unwrap_or(0),
            altitude: data.angle.map(|angle| angle.altitude).unwrap_or(0.0),
            azimuth: data.angle.map(|angle| angle.azimuth).unwrap_or(0.0),
        },
        _ => {
            #[cfg(feature = "tracing")]
            tracing::error!("unsupported PointerSource variant: {source:?}");
            PointerDetails::default()
        }
    }
}

pub(crate) fn winit_modifiers_to_kbt_modifiers(winit_modifiers: WinitModifiers) -> Modifiers {
    let mut modifiers = Modifiers::default();
    if winit_modifiers.control_key() {
        modifiers.insert(Modifiers::CONTROL);
    }
    if winit_modifiers.alt_key() {
        modifiers.insert(Modifiers::ALT);
    }
    if winit_modifiers.shift_key() {
        modifiers.insert(Modifiers::SHIFT);
    }
    if winit_modifiers.meta_key() {
        modifiers.insert(Modifiers::META);
    }
    modifiers
}

pub(crate) fn winit_physical_key_to_kbt_code(physical_key: &WinitPhysicalKey) -> Code {
    match physical_key {
        WinitPhysicalKey::Code(code) => *code,
        WinitPhysicalKey::Unidentified(_) => Code::Unidentified,
    }
}

pub(crate) fn winit_key_to_kbt_key(winit_key: &WinitKey) -> Key {
    match winit_key {
        WinitKey::Named(named_key) => Key::Named(*named_key),
        WinitKey::Character(c) => Key::Character(c.to_string()),
        WinitKey::Unidentified(_) => Key::Named(NamedKey::Unidentified),
        WinitKey::Dead(_) => Key::Named(NamedKey::Dead),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_ids_are_stable_while_active_and_fresh_after_leaving() {
        let mut ids = PointerIds::default();
        let a = FingerId::from_raw(0xdead_beef);
        let b = FingerId::from_raw(7);

        assert_eq!(ids.get_or_assign(a), BlitzPointerId::finger(0));
        assert_eq!(ids.get_or_assign(b), BlitzPointerId::finger(1));
        assert_eq!(ids.get_or_assign(a), BlitzPointerId::finger(0));

        // A raw id the platform reuses after the finger left is a new finger
        ids.remove(a);
        assert_eq!(ids.get_or_assign(a), BlitzPointerId::finger(2));
    }
}
