use serde_json::json;

use crate::GenericClientMessage;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext};

use super::generate_name;

pub(crate) struct DeviceActor {
    name: String,
}

impl DeviceActor {
    pub(crate) fn new() -> Self {
        Self {
            name: generate_name("device"),
        }
    }
}

impl Actor for DeviceActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            "getDescription" => {
                ctx.write_msg(
                    self.name(),
                    json!({ "value": {
                        "apptype":"blitz",
                        "name":"Blitz",
                        "brandName":"Blitz",
                        "version": env!("CARGO_PKG_VERSION").to_string(),
                        "appbuildid": format!("Version {}", env!("CARGO_PKG_VERSION").to_string()),
                        "platformversion":"135.0",
                        // "platformbuildid":"Version 1.0",
                        // "useragent":"Mozilla/5.0 (macOS; AArch64) Ladybird/1.0",
                        // "os":"macOS",
                        // "arch":"AArch64"
                    }}),
                );
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}
