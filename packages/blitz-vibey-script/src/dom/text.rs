//! The `Text` layer — a text node under `CharacterData`.
//!
//! The textual-data API (`data`, `nodeValue`, ...) lives on the parent
//! layers; `Text` only fixes the class identity.

use boa_engine::class::ClassBuilder;
use boa_engine::{Context, Finalize, JsData, JsResult, Trace};

use crate::shared::{ExtendLayer, Extended};

use super::character_data::CharacterDataLayer;

/// `Text` own block.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct TextLayer;

pub(crate) type Text = Extended<TextLayer>;

impl ExtendLayer for TextLayer {
    type Parent = CharacterDataLayer;
    const CLASS_NAME: &'static str = "Text";

    fn define_members(_class: &mut ClassBuilder<'_>) -> JsResult<()> {
        Ok(())
    }
}

/// Register the `Text` class and wire up the `Text -> CharacterData`
/// prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<Text>()?;
    crate::shared::link_prototype::<Text>(context)?;
    Ok(())
}
