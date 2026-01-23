use serde_json::json;

use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext};
use crate::{GenericClientMessage, JsonValue};

use super::generate_name;

pub(crate) struct ProcessActor {
    name: String,
}

impl ProcessActor {
    pub(crate) fn new() -> Self {
        Self {
            name: generate_name("process"),
        }
    }

    pub(crate) fn description(&self) -> JsonValue {
        json!({
            "actor": self.name(),
            "id": 0, // TODO: track ID
            "isParent": true,
            "isWindowlessParent":false,
            "traits": {
                "watcher":true,
                "supportsReloadDescriptor":true
            }
        })
    }
}

impl Actor for ProcessActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &self,
        _ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}
