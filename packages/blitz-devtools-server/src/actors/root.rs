use serde_json::json;

use crate::GenericClientMessage;
use crate::actors::process::ProcessActor;
use crate::actors::tab::TabDescriptorActor;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext};

pub(crate) struct RootActor {
    preference_actor_name: String,
    device_actor_name: String,
    process_actor_name: String,
    /// Map from document id to tab descriptor actor name
    tabs: Vec<(usize, String)>,
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
            tabs: Vec::new(),
        }
    }

    /// Get (creating if necessary) the tab descriptor actor for a document id
    fn tab_actor_name(&mut self, ctx: &mut DevtoolContext<'_>, doc_id: usize) -> String {
        if let Some((_, name)) = self.tabs.iter().find(|(id, _)| *id == doc_id) {
            return name.clone();
        }
        let tab = TabDescriptorActor::new(doc_id);
        let name = tab.name();
        self.tabs.push((doc_id, name.clone()));
        ctx.push_actor(Box::new(tab));
        name
    }
}

impl Actor for RootActor {
    fn name(&self) -> ActorId {
        String::from("root")
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            "connect" => {
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
                let doc_ids = ctx.docs.document_ids();
                let mut tabs = Vec::with_capacity(doc_ids.len());
                for (idx, doc_id) in doc_ids.into_iter().enumerate() {
                    let actor_name = self.tab_actor_name(ctx, doc_id);
                    if let Some(form) =
                        TabDescriptorActor::form_for(ctx, &actor_name, doc_id, idx == 0)
                    {
                        tabs.push(form);
                    }
                }
                ctx.write_msg(self.name(), json!({ "tabs": tabs }));
                Ok(())
            }
            "getTab" => {
                let msg = message.data.json()?;
                let browser_id =
                    msg.get("browserId")
                        .and_then(|v| v.as_u64())
                        .ok_or(ActorMessageErr::MissingParameter)? as usize;
                let actor_name = self.tab_actor_name(ctx, browser_id);
                let form = TabDescriptorActor::form_for(ctx, &actor_name, browser_id, true)
                    .ok_or(ActorMessageErr::NoSuchDocument)?;
                ctx.write_msg(self.name(), json!({ "tab": form }));
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
                let description = process.description();
                ctx.write_msg(self.name(), json!({ "processes": [description] }));
                Ok(())
            }

            "getProcess" => {
                let process = ctx.actor::<ProcessActor>(&self.process_actor_name);
                let description = process.description();
                ctx.write_msg(self.name(), json!({ "processDescriptor": description }));
                Ok(())
            }

            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}
