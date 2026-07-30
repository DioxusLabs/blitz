use blitz_dom::BaseDocument;
use serde_json::json;

use crate::actors::watcher::{SessionNames, WatcherActor};
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext, generate_name};
use crate::{GenericClientMessage, JsonValue};

/// Descriptor actor representing a single Blitz document ("tab")
pub(crate) struct TabDescriptorActor {
    name: String,
    doc_id: usize,
    watcher_name: Option<String>,
    session: Option<SessionNames>,
}

pub(crate) fn doc_title(doc: &BaseDocument) -> String {
    doc.find_title_node()
        .map(|node| node.text_content())
        .unwrap_or_default()
}

impl TabDescriptorActor {
    pub(crate) fn new(doc_id: usize) -> Self {
        Self {
            name: generate_name("tab"),
            doc_id,
            watcher_name: None,
            session: None,
        }
    }

    pub(crate) fn form_for(
        ctx: &mut DevtoolContext<'_>,
        actor_name: &str,
        doc_id: usize,
        selected: bool,
    ) -> Option<JsonValue> {
        let actor_name = actor_name.to_string();
        ctx.with_doc(doc_id, |doc| {
            json!({
                "actor": actor_name,
                "title": doc_title(doc),
                "url": doc.url().to_string(),
                "browserId": doc_id,
                "browsingContextID": doc_id,
                "outerWindowID": doc_id,
                "isZombieTab": false,
                "selected": selected,
                "traits": {
                    "watcher": true,
                    "supportsReloadDescriptor": false,
                },
            })
        })
    }

    fn ensure_watcher(&mut self, ctx: &mut DevtoolContext<'_>) -> (String, SessionNames) {
        if let (Some(name), Some(session)) = (&self.watcher_name, &self.session) {
            return (name.clone(), session.clone());
        }
        let watcher = WatcherActor::create(ctx, self.doc_id);
        let name = watcher.name();
        let session = watcher.session.clone();
        self.watcher_name = Some(name.clone());
        self.session = Some(session.clone());
        ctx.push_actor(Box::new(watcher));
        (name, session)
    }
}

impl Actor for TabDescriptorActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            "getTarget" => {
                let (_, session) = self.ensure_watcher(ctx);
                let form = WatcherActor::frame_target_form(ctx, self.doc_id, &session)
                    .ok_or(ActorMessageErr::NoSuchDocument)?;
                ctx.write_msg(self.name(), json!({ "frame": form }));
                Ok(())
            }
            "getFavicon" => {
                ctx.write_msg(self.name(), json!({ "favicon": "" }));
                Ok(())
            }
            "getWatcher" => {
                let (watcher_name, _) = self.ensure_watcher(ctx);
                ctx.write_msg(
                    self.name(),
                    json!({
                        "actor": watcher_name,
                        "traits": {
                            "resources": {},
                            "frame": true,
                        },
                    }),
                );
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}
