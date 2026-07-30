use serde_json::json;
use style::properties::{LonghandId, NonCustomPropertyId, PropertyId, ShorthandId};

use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext, generate_name};
use crate::{GenericClientMessage, JsonValue};

/// Provides the client with the database of supported CSS properties,
/// generated from Stylo's property definitions.
pub(crate) struct CssPropertiesActor {
    name: String,
}

impl CssPropertiesActor {
    pub(crate) fn new() -> Self {
        Self {
            name: generate_name("css-properties"),
        }
    }

    fn css_database() -> JsonValue {
        let mut properties = serde_json::Map::new();
        for id in NonCustomPropertyId::iter() {
            // Skip aliases and internal/pref-disabled properties
            if id.as_alias().is_some() {
                continue;
            }
            if !PropertyId::NonCustom(id).enabled_for_all_content() {
                continue;
            }

            let (is_inherited, subproperties): (bool, Vec<&'static str>) =
                match id.longhand_or_shorthand() {
                    Ok(longhand) => (longhand.inherited(), vec![longhand.name()]),
                    Err(shorthand) => (
                        shorthand_inherited(shorthand),
                        shorthand.longhands().map(|l| l.name()).collect(),
                    ),
                };

            properties.insert(
                id.name().to_string(),
                json!({
                    "isInherited": is_inherited,
                    "values": [],
                    "supports": [],
                    "subproperties": subproperties,
                }),
            );
        }
        JsonValue::Object(properties)
    }
}

/// A shorthand is considered inherited if all of its longhands are inherited
fn shorthand_inherited(shorthand: ShorthandId) -> bool {
    shorthand.longhands().all(LonghandId::inherited)
}

impl Actor for CssPropertiesActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            "getCSSDatabase" => {
                ctx.write_msg(self.name(), json!({ "properties": Self::css_database() }));
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}
