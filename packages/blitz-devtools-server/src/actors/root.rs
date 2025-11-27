use serde_json::json;

use crate::GenericClientMessage;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext};

pub(crate) struct RootActor {
    preference_actor_name: String,
    device_actor_name: String,
}

impl RootActor {
    pub(crate) fn new(preference_actor_name: String, device_actor_name: String) -> Self {
        Self {
            preference_actor_name,
            device_actor_name,
        }
    }
}

impl Actor for RootActor {
    fn name(&self) -> ActorId {
        String::from("root")
    }

    fn handle_message(
        &self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            "connect" => {
                println!("CONNECT");
                ctx.write_msg(self.name(), json!({}));
                Ok(())
            }
            "getRoot" => {
                ctx.write_msg(
                    self.name(),
                    json!({
                      "selected": 0,
                      "deviceActor": self.device_actor_name.clone(),
                      "preferenceActor": self.preference_actor_name.clone(),
                    }),
                );
                Ok(())
            }
            "listTabs" => Ok(()),
            "listWorkers" => {
                ctx.write_msg(self.name(), json!({ "workers": [] }));
                Ok(())
            }
            "listServiceWorkerRegistrations" => {
                ctx.write_msg(self.name(), json!({ "registrations": [] }));
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}
