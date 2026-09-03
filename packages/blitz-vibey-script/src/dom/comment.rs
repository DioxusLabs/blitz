//! The `Comment` layer — a comment node under `CharacterData`.
//!
//! blitz retains no comment payload, so the standard-visible value reads
//! back as an empty string (via the parent layers' accessors); `Comment`
//! only fixes the class identity.

use boa_engine::class::ClassBuilder;
use boa_engine::{Context, Finalize, JsData, JsResult, Trace};

use crate::shared::{ExtendLayer, Extended};

use super::character_data::CharacterDataLayer;

/// `Comment` own block.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct CommentLayer;

pub(crate) type Comment = Extended<CommentLayer>;

impl ExtendLayer for CommentLayer {
    type Parent = CharacterDataLayer;
    const CLASS_NAME: &'static str = "Comment";

    fn define_members(_class: &mut ClassBuilder<'_>) -> JsResult<()> {
        Ok(())
    }
}

/// Register the `Comment` class and wire up the `Comment -> CharacterData`
/// prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<Comment>()?;
    crate::shared::link_prototype::<Comment>(context)?;
    Ok(())
}
