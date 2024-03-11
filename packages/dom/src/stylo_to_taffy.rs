// Module of type aliases so we can refer to stylo types with nicer names
mod stylo {
    pub(crate) use style::computed_values::align_content::T as AlignContent;
    pub(crate) use style::computed_values::align_items::T as AlignItems;
    pub(crate) use style::computed_values::align_self::T as AlignSelf;
    pub(crate) use style::computed_values::flex_direction::T as FlexDirection;
    pub(crate) use style::computed_values::justify_content::T as JustifyContent;
    // pub(crate) use style::computed_values::justify_items::T as JustifyItems;
    // pub(crate) use style::computed_values::justify_self::T as JustifySelf;
}

pub(crate) fn flex_direction(input: stylo::FlexDirection) -> taffy::FlexDirection {
    match input {
        stylo::FlexDirection::Row => taffy::FlexDirection::Row,
        stylo::FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
        stylo::FlexDirection::Column => taffy::FlexDirection::Column,
        stylo::FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
    }
}

pub(crate) fn justify_content(input: stylo::JustifyContent) -> Option<taffy::JustifyContent> {
    match input {
        stylo::JustifyContent::FlexStart => Some(taffy::JustifyContent::FlexStart),
        stylo::JustifyContent::Stretch => Some(taffy::JustifyContent::Stretch),
        stylo::JustifyContent::FlexEnd => Some(taffy::JustifyContent::FlexEnd),
        stylo::JustifyContent::Center => Some(taffy::JustifyContent::Center),
        stylo::JustifyContent::SpaceBetween => Some(taffy::JustifyContent::SpaceBetween),
        stylo::JustifyContent::SpaceAround => Some(taffy::JustifyContent::SpaceAround),
    }
}

pub(crate) fn align_content(input: stylo::AlignContent) -> Option<taffy::AlignContent> {
    match input {
        stylo::AlignContent::FlexStart => Some(taffy::AlignContent::FlexStart),
        stylo::AlignContent::Stretch => Some(taffy::AlignContent::Stretch),
        stylo::AlignContent::FlexEnd => Some(taffy::AlignContent::FlexEnd),
        stylo::AlignContent::Center => Some(taffy::AlignContent::Center),
        stylo::AlignContent::SpaceBetween => Some(taffy::AlignContent::SpaceBetween),
        stylo::AlignContent::SpaceAround => Some(taffy::AlignContent::SpaceAround),
    }
}

pub(crate) fn align_items(input: stylo::AlignItems) -> Option<taffy::AlignItems> {
    match input {
        stylo::AlignItems::Stretch => Some(taffy::AlignItems::Stretch),
        stylo::AlignItems::FlexStart => Some(taffy::AlignItems::FlexStart),
        stylo::AlignItems::FlexEnd => Some(taffy::AlignItems::FlexEnd),
        stylo::AlignItems::Center => Some(taffy::AlignItems::Center),
        stylo::AlignItems::Baseline => Some(taffy::AlignItems::Baseline),
    }
}

pub(crate) fn align_self(input: stylo::AlignSelf) -> Option<taffy::AlignSelf> {
    match input {
        stylo::AlignSelf::Auto => None,
        stylo::AlignSelf::Stretch => Some(taffy::AlignSelf::Stretch),
        stylo::AlignSelf::FlexStart => Some(taffy::AlignSelf::FlexStart),
        stylo::AlignSelf::FlexEnd => Some(taffy::AlignSelf::FlexEnd),
        stylo::AlignSelf::Center => Some(taffy::AlignSelf::Center),
        stylo::AlignSelf::Baseline => Some(taffy::AlignSelf::Baseline),
    }
}

// pub(crate) fn justify_items(input: stylo::JustifyItems) -> Option<taffy::JustifyItems> {
//     match input {
//         stylo::JustifyItems::Stretch => Some(taffy::JustifyItems::Stretch),
//         stylo::JustifyItems::FlexStart => Some(taffy::JustifyItems::FlexStart),
//         stylo::JustifyItems::FlexEnd => Some(taffy::JustifyItems::FlexEnd),
//         stylo::JustifyItems::Center => Some(taffy::JustifyItems::Center),
//         stylo::JustifyItems::Baseline => Some(taffy::JustifyItems::Baseline),
//     }
// }

// pub(crate) fn justify_self(input: stylo::JustifySelf) -> Option<taffy::JustifySelf> {
//     match input {
//         stylo::JustifySelf::Auto => None,
//         stylo::JustifySelf::Stretch => Some(taffy::JustifySelf::Stretch),
//         stylo::JustifySelf::FlexStart => Some(taffy::JustifySelf::FlexStart),
//         stylo::JustifySelf::FlexEnd => Some(taffy::JustifySelf::FlexEnd),
//         stylo::JustifySelf::Center => Some(taffy::JustifySelf::Center),
//         stylo::JustifySelf::Baseline => Some(taffy::JustifySelf::Baseline),
//     }
// }
