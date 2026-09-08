//! The `CharacterData` class: the textual-data API shared by `Text` and
//! `Comment`.

use boa_engine::class::ClassBuilder;
use boa_engine::property::Attribute;
use boa_engine::{Context, Finalize, JsData, JsResult, Trace};

use crate::shared::{ExtendLayer, Extended, instance_accessor, js_fn_ptr};

use super::node::{node_value, set_node_value};

/// `CharacterData` own block. All data lives in the `Node` layer; this layer
/// only contributes the `data` accessor to the prototype chain.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct CharacterDataLayer;

pub(crate) type CharacterData = Extended<CharacterDataLayer>;

impl ExtendLayer for CharacterDataLayer {
    type Parent = super::node::NodeLayer;
    const CLASS_NAME: &'static str = "CharacterData";

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        instance_accessor!(
            class,
            "data",
            js_fn_ptr!(node_value, &realm),
            js_fn_ptr!(set_node_value, &realm),
            attr
        );

        Ok(())
    }
}

/// Register the `CharacterData` class and wire up the
/// `CharacterData -> Node` prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<CharacterData>()?;
    crate::shared::link_prototype::<CharacterData>(context)?;
    Ok(())
}
