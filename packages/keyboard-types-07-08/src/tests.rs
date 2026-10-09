use crate::test_names::{CODE_NAMES, KEY_NAMES};
use crate::{to_07, to_08};

/// Round-trip every v0.7 `Key` variant through v0.8 and back, and check that
/// the v0.8 representation has the same string representation.
#[test]
fn key_round_trip() {
    let mut count = 0;
    for name in KEY_NAMES {
        let key07: kt07::Key = name.parse().unwrap();
        let key08 = to_08::key(key07.clone());
        assert_eq!(key08.to_string(), *name);
        assert_eq!(to_07::key(key08), key07);
        count += 1;
    }
    assert!(count > 300);
}

#[test]
fn character_key() {
    let key07 = kt07::Key::Character("a".to_string());
    let key08 = to_08::key(key07.clone());
    assert_eq!(key08, kt08::Key::Character("a".to_string()));
    assert_eq!(to_07::key(key08), key07);
}

/// Round-trip every v0.7 `Code` variant through v0.8 and back, and check that
/// the v0.8 representation has the same string representation.
#[test]
fn code_round_trip() {
    let mut count = 0;
    for name in CODE_NAMES {
        let Ok(code07) = name.parse::<kt07::Code>() else {
            // Codes which only exist in v0.8 map to `Unidentified` in v0.7
            let code08: kt08::Code = name.parse().unwrap();
            assert_eq!(to_07::code(code08), kt07::Code::Unidentified);
            continue;
        };
        let code08 = to_08::code(code07);
        assert_eq!(code08.to_string(), *name);
        assert_eq!(to_07::code(code08), code07);
        count += 1;
    }
    assert!(count > 200);
}

#[test]
fn modifiers() {
    let all07 = kt07::Modifiers::all();
    let all08 = kt08::Modifiers::all();
    assert_eq!(to_08::modifiers(all07), all08);
    assert_eq!(to_07::modifiers(all08), all07);
    assert_eq!(
        to_08::modifiers(kt07::Modifiers::CONTROL | kt07::Modifiers::SHIFT),
        kt08::Modifiers::CONTROL | kt08::Modifiers::SHIFT
    );
}

#[test]
fn keyboard_event_round_trip() {
    let event07 = kt07::KeyboardEvent {
        state: kt07::KeyState::Down,
        key: kt07::Key::Enter,
        code: kt07::Code::NumpadEnter,
        location: kt07::Location::Numpad,
        modifiers: kt07::Modifiers::ALT,
        repeat: true,
        is_composing: false,
    };
    let event08 = to_08::keyboard_event(event07.clone());
    assert_eq!(event08.key, kt08::Key::Named(kt08::NamedKey::Enter));
    assert_eq!(event08.code, kt08::Code::NumpadEnter);
    assert_eq!(to_07::keyboard_event(event08), event07);
}

#[test]
fn composition_event_round_trip() {
    let event07 = kt07::CompositionEvent {
        state: kt07::CompositionState::Update,
        data: "あ".to_string(),
    };
    let event08 = to_08::composition_event(event07.clone());
    assert_eq!(event08.state, kt08::CompositionState::Update);
    assert_eq!(to_07::composition_event(event08), event07);
}
