use serde_json::json;

use crate::GenericClientMessage;
use crate::actors::process::ProcessActor;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext};

pub(crate) struct RootActor {
    preference_actor_name: String,
    device_actor_name: String,
    process_actor_name: String,
}

impl RootActor {
    pub(crate) fn new(
        preference_actor_name: String,
        device_actor_name: String,
        process_actor_name: String,
    ) -> Self {
        Self {
            preference_actor_name,
            device_actor_name,
            process_actor_name,
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
            "listTabs" => {
                ctx.write_msg(self.name(), json!({ "tabs": [] }));
                Ok(())
            }
            "listWorkers" => {
                ctx.write_msg(self.name(), json!({ "workers": [] }));
                Ok(())
            }
            "listAddons" => {
                ctx.write_msg(self.name(), json!({ "addons": [] }));
                Ok(())
            }
            "listServiceWorkerRegistrations" => {
                ctx.write_msg(self.name(), json!({ "registrations": [] }));
                Ok(())
            }

            "listProcesses" => {
                let process = ctx.actor::<ProcessActor>(&self.process_actor_name);
                ctx.write_msg(self.name(), json!({ "processes": [process.description()] }));
                Ok(())
            }

            "getProcess" => {
                let process = ctx.actor::<ProcessActor>(&self.process_actor_name);
                ctx.write_msg(
                    self.name(),
                    json!({ "processDescriptor": process.description() }),
                );
                Ok(())
            }

            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}
