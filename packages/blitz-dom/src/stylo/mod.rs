//! Enable the dom to participate in styling by servo
//!

mod device;
mod dom;
mod prefs;
mod presentational_hints;
mod traversal;
mod user_agent;

pub(crate) use device::{DeviceChanges, make_device, make_media_data};
pub(crate) use dom::BlitzNode;
pub(crate) use prefs::init_stylo_prefs;
pub(crate) use presentational_hints::PresentationalHints;
pub use user_agent::UserAgentStylesheet;
