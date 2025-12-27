use serde_json::json;

use crate::GenericClientMessage;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext};

use super::generate_name;

pub(crate) struct PreferenceActor {
    name: String,
}

impl PreferenceActor {
    pub(crate) fn new() -> Self {
        Self {
            name: generate_name("preference"),
        }
    }
}

impl Actor for PreferenceActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            "getBoolPref" => {
                ctx.write_msg(self.name(), json!({ "value": false }));
                Ok(())
            }
            "getIntPref" => {
                ctx.write_msg(self.name(), json!({ "value": 0 }));
                Ok(())
            }
            "getFloatPref" => {
                ctx.write_msg(self.name(), json!({ "value": 0.0 }));
                Ok(())
            }
            "getCharPref" => {
                ctx.write_msg(self.name(), json!({ "value": "" }));
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}
