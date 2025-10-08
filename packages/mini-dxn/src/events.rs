use std::any::Any;

use blitz_traits::events::BlitzKeyEvent;
use dioxus_html::{
    AnimationData, ClipboardData, CompositionData, DragData, FocusData, FormData, FormValue,
    HasFileData, HasFormData, HasKeyboardData, HasMouseData, HtmlEventConverter, ImageData,
    KeyboardData, MediaData, MountedData, MouseData, PlatformEventData, PointerData, ResizeData,
    ScrollData, SelectionData, ToggleData, TouchData, TransitionData, VisibleData, WheelData,
    geometry::{ClientPoint, ElementPoint, PagePoint, ScreenPoint},
    input_data::{MouseButton, MouseButtonSet},
    point_interaction::{
        InteractionElementOffset, InteractionLocation, ModifiersInteraction, PointerInteraction,
    },
};
use keyboard_types::{Code, Key, Location, Modifiers};

#[derive(Clone)]
pub struct NativeClickData;

impl InteractionLocation for NativeClickData {
    fn client_coordinates(&self) -> ClientPoint {
        todo!()
    }

    fn screen_coordinates(&self) -> ScreenPoint {
        todo!()
    }

    fn page_coordinates(&self) -> PagePoint {
        todo!()
    }
}
impl InteractionElementOffset for NativeClickData {
    fn element_coordinates(&self) -> ElementPoint {
        todo!()
    }
}
impl ModifiersInteraction for NativeClickData {
    fn modifiers(&self) -> Modifiers {
        todo!()
    }
}

impl PointerInteraction for NativeClickData {
    fn trigger_button(&self) -> Option<MouseButton> {
        todo!()
    }

    fn held_buttons(&self) -> MouseButtonSet {
        todo!()
    }
}
impl HasMouseData for NativeClickData {
    fn as_any(&self) -> &dyn std::any::Any {
        self as &dyn std::any::Any
    }
}

pub struct NativeConverter {}

impl HtmlEventConverter for NativeConverter {
    fn convert_animation_data(&self, _event: &PlatformEventData) -> AnimationData {
        todo!()
    }

    fn convert_clipboard_data(&self, _event: &PlatformEventData) -> ClipboardData {
        todo!()
    }

    fn convert_composition_data(&self, _event: &PlatformEventData) -> CompositionData {
        todo!()
    }

    fn convert_drag_data(&self, _event: &PlatformEventData) -> DragData {
        todo!()
    }

    fn convert_focus_data(&self, _event: &PlatformEventData) -> FocusData {
        todo!()
    }

    fn convert_form_data(&self, event: &PlatformEventData) -> FormData {
        let o = event.downcast::<NativeFormData>().unwrap().clone();
        FormData::from(o)
    }

    fn convert_image_data(&self, _event: &PlatformEventData) -> ImageData {
        todo!()
    }

    fn convert_keyboard_data(&self, event: &PlatformEventData) -> KeyboardData {
        let data = event.downcast::<BlitzKeyboardData>().unwrap().clone();
        KeyboardData::from(data)
    }

    fn convert_media_data(&self, _event: &PlatformEventData) -> MediaData {
        todo!()
    }

    fn convert_mounted_data(&self, _event: &PlatformEventData) -> MountedData {
        todo!()
    }

    fn convert_mouse_data(&self, event: &PlatformEventData) -> MouseData {
        let o = event.downcast::<NativeClickData>().unwrap().clone();
        MouseData::from(o)
    }

    fn convert_pointer_data(&self, _event: &PlatformEventData) -> PointerData {
        todo!()
    }

    fn convert_scroll_data(&self, _event: &PlatformEventData) -> ScrollData {
        todo!()
    }

    fn convert_selection_data(&self, _event: &PlatformEventData) -> SelectionData {
        todo!()
    }

    fn convert_toggle_data(&self, _event: &PlatformEventData) -> ToggleData {
        todo!()
    }

    fn convert_touch_data(&self, _event: &PlatformEventData) -> TouchData {
        todo!()
    }

    fn convert_transition_data(&self, _event: &PlatformEventData) -> TransitionData {
        todo!()
    }

    fn convert_wheel_data(&self, _event: &PlatformEventData) -> WheelData {
        todo!()
    }

    fn convert_resize_data(&self, _event: &PlatformEventData) -> ResizeData {
        todo!()
    }

    fn convert_visible_data(&self, _event: &PlatformEventData) -> VisibleData {
        todo!()
    }

    fn convert_cancel_data(&self, _event: &PlatformEventData) -> dioxus_html::CancelData {
        todo!()
    }
}

#[derive(Clone, Debug)]
pub struct NativeFormData {
    pub value: String,
    pub values: Vec<(String, FormValue)>,
}

impl HasFormData for NativeFormData {
    fn as_any(&self) -> &dyn std::any::Any {
        self as &dyn std::any::Any
    }

    fn value(&self) -> String {
        self.value.clone()
    }

    fn values(&self) -> Vec<(String, FormValue)> {
        self.values.clone()
    }

    fn valid(&self) -> bool {
        true
    }
}

impl HasFileData for NativeFormData {
    fn files(&self) -> Vec<dioxus_html::FileData> {
        Vec::new()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BlitzKeyboardData(pub(crate) BlitzKeyEvent);

impl ModifiersInteraction for BlitzKeyboardData {
    fn modifiers(&self) -> Modifiers {
        self.0.modifiers
    }
}

impl HasKeyboardData for BlitzKeyboardData {
    fn key(&self) -> Key {
        self.0.key.clone()
    }

    fn code(&self) -> Code {
        self.0.code
    }

    fn location(&self) -> Location {
        self.0.location
    }

    fn is_auto_repeating(&self) -> bool {
        self.0.is_auto_repeating
    }

    fn is_composing(&self) -> bool {
        self.0.is_composing
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self as &dyn Any
    }
}
